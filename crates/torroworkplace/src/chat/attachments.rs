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
//! - **Link cards** (issue #785): each of the first three `https` links of a message
//!   ([`crate::link_cards::links`]) may have one, by its `index` among them. Links to the account's own
//!   server are resolved by themselves when an app reads messages. Any other follows Settings › Privacy
//!   (issue #672, [`crate::link_cards::web`]), because the server fetches the address and its owner
//!   might learn when the message was read: by itself for a provider allowed or at "Always show"; for
//!   one not asked yet the message names it (`foreign`, the first such link) and the card comes with
//!   `chat.show_card`, a person's click ("Show once"); never for one blocked or at "Never show", where a
//!   card held from before is left out too. A client's read resolves nothing.
//! - **YouTube videos** (issue #673): the card of a link to a video carries its id (`video`), also
//!   when the server knows no card for it; so the chat window can play exactly what Settings ›
//!   Privacy let load, and a person's "Show once" counts for playing that video too.

use std::collections::{BTreeMap, VecDeque};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use chrono::Local;
use torro_chat::{ChatError, ForeignProvider, MessagePage, split_message};
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
use crate::account::github::GithubAccounts;
use crate::link_cards::web::{self, CardPicture, Decided, Resolved};
use crate::link_cards::{self, LinkKind};
use crate::pim::Reach;

/// The most bytes of previews kept per account.
pub const PREVIEWS_PER_ACCOUNT: u64 = 64 * 1024 * 1024;

/// The most messages whose link cards are kept.
pub const CARDS_KEPT: usize = 2000;

/// The most cards resolved by one read of an app: links to the own server and to providers allowed.
const CARDS_PER_READ: usize = 5;

/// How long a fetched file stays in the chat's folder.
pub const FETCHED_FOR: Duration = Duration::from_secs(24 * 60 * 60);

/// Previews and link cards in memory.
#[derive(Debug, Default)]
pub struct Held {
    /// Per account: the previews by key (`<file id>:<edge>`), the oldest first, and their bytes.
    previews: BTreeMap<String, (VecDeque<(String, Picture)>, u64)>,
    /// By message id, the oldest first: the cards of its links by their index among them
    /// ([`link_cards::links`]), each with the link it was resolved for, `None` for a link not resolved (yet). A
    /// message edited since may have another link at that index; its card is then not held ([`Held::card`]).
    cards: BTreeMap<String, Vec<Option<(String, Resolved)>>>,
    order: VecDeque<String>,
    /// GitHub links no connected account showed an item for (issue #784), by link, the oldest first, with when and
    /// whether GitHub could not be asked then: asked again after [`link_cards::NO_ITEM_AFTER`] (an account connected
    /// later, a token that reaches the repository now), after [`link_cards::RETRY_AFTER`] when it failed. So such a
    /// link is not asked with every read; meanwhile it is the page it is.
    no_items: BTreeMap<String, (Instant, bool)>,
    no_item_order: VecDeque<String>,
    /// The connected GitHub accounts, for the cards of issues and pull requests (issue #790); `None` until the core
    /// hands them over ([`ChatService::set_github`]), and then such a link is the page it is.
    github: Option<Github>,
}

/// The GitHub accounts as [`Held`] keeps them; they tell nothing of themselves in a debug print.
#[derive(Clone)]
struct Github(Arc<GithubAccounts>);

impl std::fmt::Debug for Github {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Github")
    }
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

    /// The card held for the link `index` of `message` when that link is `link`: one resolved for another link (the
    /// message was edited since, review of #839) is not held, so neither its card nor its picture shows under the
    /// new link and the rights checked for it.
    fn card(&self, message: &str, index: usize, link: &str) -> Option<&Resolved> {
        let (held, card) = self.cards.get(message)?.get(index)?.as_ref()?;
        (held == link).then_some(card)
    }

    /// Keeps `card` for the link `index` (below [`link_cards::MAX_LINKS`]) of `message`, which is `link`, its card
    /// carrying that `index` (issue #785: the window takes it as it is); the oldest messages go beyond
    /// [`CARDS_KEPT`].
    fn keep_card(&mut self, message: &str, index: usize, link: &str, mut card: Resolved) {
        let Some(at) = u8::try_from(index).ok().filter(|_| index < link_cards::MAX_LINKS) else { return };
        if let Some(card) = card.card.as_mut() {
            card.index = at;
        }
        if !self.cards.contains_key(message) {
            self.order.push_back(message.to_string());
        }
        let slots = self.cards.entry(message.to_string()).or_insert_with(|| vec![None; link_cards::MAX_LINKS]);
        slots[index] = Some((link.to_owned(), card));
        while self.order.len() > CARDS_KEPT {
            if let Some(oldest) = self.order.pop_front() {
                self.cards.remove(&oldest);
            }
        }
    }

    /// Whether GitHub is not to be asked about `link` at `now` ([`Held::no_items`]).
    fn no_item(&self, link: &str, now: Instant) -> bool {
        match self.no_items.get(link) {
            Some((at, true)) => now.duration_since(*at) < link_cards::RETRY_AFTER,
            Some((at, false)) => now.duration_since(*at) < link_cards::NO_ITEM_AFTER,
            None => false,
        }
    }

    /// Keeps that no account showed an item for `link` at `at` (`failed` when GitHub could not be asked); the oldest
    /// go beyond [`CARDS_KEPT`].
    fn keep_no_item(&mut self, link: &str, at: Instant, failed: bool) {
        if self.no_items.insert(link.to_owned(), (at, failed)).is_none() {
            self.no_item_order.push_back(link.to_owned());
        }
        while self.no_item_order.len() > CARDS_KEPT {
            if let Some(oldest) = self.no_item_order.pop_front() {
                self.no_items.remove(&oldest);
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

/// What the link `link` of a message is for its card, `server` being the account's own server (`None` signed out):
/// its kind ([`link_cards::classify`]) and, for a page, [`Decided::Own`] or what Settings › Privacy decides
/// ([`web::decide`]); a GitHub item that no connected account shows is such a page too.
fn decided(privacy: &torro_core::privacy::Privacy, link: &str, server: Option<&str>) -> (LinkKind, Decided) {
    (link_cards::classify(link, server), web::decide(privacy, link, server))
}

/// Whether `reach` may get the card `card` the core holds for the link of kind `kind` (`decided` for a page) in a
/// message of the chat account `account` (issue #784): cards are resolved with the person's rights, so a client gets
/// one only with the right its kind asks for itself – a page (also a video, a GitHub link no account shows) while
/// Settings › Privacy does not turn it off; a file of the own server with the right to its files
/// ([`link_cards::nextcloud::reads_files`]); an issue or pull request with read access to its repository
/// ([`link_cards::github::reads_repo`]); a quoted message with the right to read its conversation
/// ([`link_cards::chat::reads_conversation`]). The app key passes every right; a page turned off is left out for it
/// too. The same rule holds for the card's picture.
fn reader_sees(
    card: &torro_chat::LinkCard,
    kind: &LinkKind,
    decided: &Decided,
    reach: Reach<'_>,
    account: &str,
) -> bool {
    use torro_chat::LinkCardKind;
    match card.kind {
        LinkCardKind::Web => *decided != Decided::Other(Decision::Skip),
        LinkCardKind::NextcloudFile => link_cards::nextcloud::reads_files(reach, account),
        LinkCardKind::Github => {
            card.github.as_ref().is_some_and(|item| link_cards::github::reads_repo(reach, &item.repo))
        }
        LinkCardKind::ChatMessage => match kind {
            LinkKind::ChatMessage { token, .. } => link_cards::chat::reads_conversation(reach, account, token),
            _ => false,
        },
    }
}

/// Which of `links` (a message's, [`link_cards::links`]) the message asks about (S2 of issue #785), `server` being
/// the account's own: the first without a card (`held` tells which have one) that is a page or a GitHub item whose
/// provider Settings › Privacy has not been asked about – the link `foreign` of `chat.get_messages` names
/// ([`ChatService::decorate`]). The first link when none asks (the provider was decided meanwhile).
fn asked_link(
    privacy: &torro_core::privacy::Privacy,
    links: &[String],
    server: Option<&str>,
    held: impl Fn(usize) -> bool,
) -> usize {
    links
        .iter()
        .enumerate()
        .position(|(at, link)| {
            let (kind, decided) = decided(privacy, link, server);
            !held(at)
                && matches!(kind, LinkKind::Web | LinkKind::Github { .. })
                && matches!(decided, Decided::Other(Decision::Ask(_)))
        })
        .unwrap_or(0)
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
    /// Hands over the connected GitHub accounts (issue #790), for the cards of links to issues and pull requests;
    /// without them such a link is the page it is. The core calls it once when it starts.
    pub fn set_github(&self, github: Arc<GithubAccounts>) {
        self.held().github = Some(Github(github));
    }

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
            return self.card_preview(id, 0, size, caller).await;
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

    /// `chat.get_preview` with `card`: the picture of the card of the link `index` (issue #785, 0 the first of
    /// the message's links, [`link_cards::links`]) of the message `id`, from the account's own server, `size` for
    /// the preview of a file (issue #790); `not-found` for a link without a card or picture the core holds, also one
    /// Settings › Privacy turned off, and for a client the picture of a card it is not shown ([`reader_sees`]).
    pub async fn card_preview(
        self: &Arc<Self>,
        id: &str,
        index: u8,
        size: PreviewSize,
        caller: Caller<'_>,
    ) -> Response {
        let Ok((account, token, number)) = super::service::message_of(id) else {
            return error_response(&ChatError::NotFound);
        };
        let (remote, caps, _) = match self.ready(account).await {
            Ok(ready) => ready,
            Err(err) => return error_response(&err),
        };
        if let Err(err) = self.room(account, &token).await {
            return error_response(&err);
        }
        if !sees_pictures(caller) {
            return no_picture_right();
        }
        // The link standing at `index` now: a card held for the link before an edit has no picture here.
        let message = match self.message(account, &token, number, &remote, &caps).await {
            Ok(message) => message,
            Err(err) => return error_response(&err),
        };
        let Some(link) = link_cards::links(&message.text).into_iter().nth(usize::from(index)) else {
            return error_response(&ChatError::NotFound);
        };
        // No picture of a card Settings › Privacy turned off (issue #672): it is left out of the messages.
        let privacy = self.privacy();
        let server = remote.server();
        let picture = self.held().card(id, usize::from(index), &link).and_then(|held| {
            let card = held.card.as_ref()?;
            let (kind, decided) = decided(&privacy, &link, Some(&server));
            reader_sees(card, &kind, &decided, caller.reach, account).then(|| held.picture.clone()).flatten()
        });
        let Some(picture) = picture else { return error_response(&ChatError::NotFound) };
        self.card_picture_of(account, &remote, &picture, size).await
    }

    /// The picture `picture` of a link card on the server of `account` (`remote`), held in memory with the
    /// account's previews: for the chat (`chat.get_preview` with `card`) and the assistant
    /// (`assistant.get_card_picture`, issue #785); a file's preview (issue #790) at `size`. Only a PNG, JPEG, GIF or
    /// WebP, never an SVG; whether the card may show is the caller's to check first.
    pub(crate) async fn card_picture_of(
        &self,
        account: &str,
        remote: &Remote,
        picture: &CardPicture,
        size: PreviewSize,
    ) -> Response {
        let key = match picture {
            CardPicture::Reference(image) => format!("card:{image}"),
            CardPicture::File(file) => format!("{file}:{}", size.edge()),
        };
        if let Some(picture) = self.held().preview(account, &key) {
            return picture_answer(picture);
        }
        let fetched = match picture {
            CardPicture::Reference(image) => remote.card_picture(image).await,
            CardPicture::File(file) => remote.preview(*file, size.edge()).await,
        };
        match fetched {
            Ok(picture) => {
                self.held().keep_preview(account, key, picture.clone());
                picture_answer(picture)
            }
            Err(err) => file_refusal(&err),
        }
    }

    /// The card of `link` (`kind`, `decided` of [`decided`]) on the server of `account` (`remote`) as the person
    /// sees it, for the chat with people (issue #785, #790): a file of the own server and a quoted message through
    /// `crate::link_cards::{nextcloud,chat}` (none when the server shows the person none: never a page card that
    /// would name it all the same); a GitHub item through the connected accounts, and when none shows it the page
    /// it is – that none does is held ([`Held::no_items`]), so GitHub is not asked with every read; a page when
    /// `page` lets it load (Settings › Privacy decided, or a person's click). `None` when it is not to load now or
    /// the server cannot be reached: asked again with the next read (GitHub a minute later at the earliest).
    async fn resolve_for_app(
        self: &Arc<Self>,
        account: &str,
        remote: &Remote,
        link: &str,
        kind: &LinkKind,
        page: bool,
    ) -> Option<Resolved> {
        let found = match kind {
            LinkKind::NextcloudFile(file) => {
                link_cards::nextcloud::resolve(self, Reach::Suite, account, link, file).await.ok()?
            }
            LinkKind::ChatMessage { token, message } => {
                link_cards::chat::resolve(self, Reach::Suite, account, link, token, *message).await.ok()?
            }
            LinkKind::Github { repo, number, .. } => {
                let now = Instant::now();
                let github = {
                    let held = self.held();
                    if held.no_item(link, now) { None } else { held.github.clone() }
                };
                let card = match github {
                    Some(Github(github)) => {
                        match link_cards::github::resolve(&github, Reach::Suite, link, repo, *number).await {
                            Ok(Some(card)) => Some(card),
                            Ok(None) => {
                                self.held().keep_no_item(link, now, false);
                                None
                            }
                            // Not reachable now: no card yet, neither the page's, which would be held for good; asked
                            // again after `RETRY_AFTER`, as the assistant does.
                            Err(_) => {
                                self.held().keep_no_item(link, now, true);
                                return None;
                            }
                        }
                    }
                    None => None,
                };
                match card {
                    Some(card) => Some(card),
                    None if page => return web::resolve(remote, link).await.ok(),
                    None => return None,
                }
            }
            LinkKind::Web if page => return web::resolve(remote, link).await.ok(),
            LinkKind::Web => return None,
        };
        Some(Resolved::found(found, kind))
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
            .flat_map(|(id, cards)| cards.iter().flatten().map(move |(_, card)| (id, card)))
            .filter_map(|(id, card)| {
                let (of, number) = split_message(id)?;
                let card = card.card.as_ref()?;
                if web::turned_off(&privacy, &card.url, server) {
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

    /// `page` of `conversation` as `caller` gets it: the cards the core holds on its messages' links
    /// (up to three each, issue #785), and for an app first the cards of links not resolved yet (at most
    /// [`CARDS_PER_READ`] per read): files of the account's own server, quoted messages and GitHub items
    /// with the person's rights (issue #790), pages of the own server and of providers Settings › Privacy
    /// allows (issue #672); and the provider of the first link not asked about yet (`foreign`). A page's
    /// card of a provider it blocks is left out for everyone; the cards of the own server and GitHub go to
    /// a client only with the right their kind asks for ([`reader_sees`]). Without the right to picture
    /// content no `blurhash`. A GitHub link no account shows is not asked again while its page may not load.
    pub(super) async fn decorate(
        self: &Arc<Self>,
        conversation: &str,
        mut page: MessagePage,
        caller: Caller<'_>,
    ) -> MessagePage {
        let privacy = self.privacy();
        let account = torro_chat::split_conversation(conversation).map(|(account, _)| account);
        let server = match account {
            Some(account) => self.remote(account).await.ok().map(|remote| (remote.server(), remote)),
            None => None,
        };
        // What each message's links are, by message id, in their order: their kind and the decision for
        // a page. Without the server (signed out) every link counts as another's page.
        let own = server.as_ref().map(|(server, _)| server.as_str());
        let links: BTreeMap<String, Vec<(String, LinkKind, Decided)>> = page
            .messages
            .iter()
            .map(|message| {
                let links = link_cards::links(&message.text).into_iter().map(|link| {
                    let (kind, decided) = decided(&privacy, &link, own);
                    (link, kind, decided)
                });
                (message.id.clone(), links.collect())
            })
            .collect();
        if matches!(caller.reach, Reach::Suite)
            && let (Some(account), Some((_, remote))) = (account, &server)
        {
            // In the order of the page and of the links in each message, as before the settings (the
            // map is ordered by the ids' text). A page only when it may load; the other kinds check their
            // own rights.
            let open: Vec<(String, usize, String, LinkKind, bool)> = {
                let held = self.held();
                let now = Instant::now();
                page.messages
                    .iter()
                    .filter_map(|message| Some((&message.id, links.get(&message.id)?)))
                    .flat_map(|(id, links)| links.iter().enumerate().map(move |(index, link)| (id, index, link)))
                    .filter(|(id, index, (link, kind, decided))| {
                        let github_held = matches!(kind, LinkKind::Github { .. }) && held.no_item(link, now);
                        let worth = match kind {
                            LinkKind::Web => decided.loads(),
                            LinkKind::Github { .. } => decided.loads() || !github_held,
                            _ => true,
                        };
                        worth && held.card(id, *index, link).is_none()
                    })
                    .map(|(id, index, (link, kind, decided))| {
                        (id.clone(), index, link.clone(), kind.clone(), decided.loads())
                    })
                    .take(CARDS_PER_READ)
                    .collect()
            };
            for (id, index, link, kind, loads) in open {
                // Not reachable now: asked again with the next read.
                if let Some(card) = self.resolve_for_app(account, remote, &link, &kind, loads).await {
                    self.held().keep_card(&id, index, &link, card);
                }
            }
        }
        let pictures = sees_pictures(caller);
        let app = matches!(caller.reach, Reach::Suite);
        let held = self.held();
        let account = account.unwrap_or_default();
        for message in &mut page.messages {
            let links = links.get(&message.id).map(Vec::as_slice).unwrap_or_default();
            let mut cards = Vec::new();
            for (index, (link, kind, decided)) in links.iter().enumerate() {
                match held.card(&message.id, index, link) {
                    Some(held) => {
                        let shown =
                            held.card.as_ref().filter(|card| reader_sees(card, kind, decided, caller.reach, account));
                        cards.extend(shown.cloned());
                    }
                    // Asked about only where a person can answer, only while no card is there, only for
                    // a page (a GitHub item no account shows is one), and only the first such link: the
                    // message names one provider.
                    None if app
                        && message.foreign.is_none()
                        && matches!(kind, LinkKind::Web | LinkKind::Github { .. }) =>
                    {
                        if let Decided::Other(Decision::Ask(provider)) = decided {
                            message.foreign =
                                Some(ForeignProvider { key: provider.key.clone(), name: provider.name.clone() });
                        }
                    }
                    None => {}
                }
            }
            if !cards.is_empty() {
                message.cards = cards;
            }
            if !pictures {
                message.files.iter_mut().for_each(|file| file.blurhash = None);
            }
        }
        drop(held);
        page
    }

    /// `chat.show_card` for the link the message `id` asks about ([`ChatService::show_card_at`] without
    /// `index`); the tests' short way.
    #[cfg(test)]
    pub async fn show_card(self: &Arc<Self>, id: &str) -> Response {
        self.show_card_at(id, None).await
    }

    /// `chat.show_card`: the card of the link `index` (issue #785, 0 the first of the message's links,
    /// [`link_cards::links`]) of the message `id`, whatever server it is on; without `index` the link the message
    /// asks about (`foreign` of `chat.get_messages`, [`asked_link`]). `not-found` for a message with fewer links,
    /// `unsupported` for a page whose provider Settings › Privacy blocks or while it says "Never show" (issue
    /// #672), `offline` while the server cannot be asked. A file of the own server, a quoted message and a GitHub
    /// item resolve as they do by themselves (issue #790), never as a page that would name them. Settings ›
    /// Privacy stays as it is; a page's card is held for every reader, with its `index`.
    pub async fn show_card_at(self: &Arc<Self>, id: &str, index: Option<u8>) -> Response {
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
        let server = remote.server();
        let links = link_cards::links(&message.text);
        let index = match index {
            Some(index) => usize::from(index),
            None => asked_link(&self.privacy(), &links, Some(&server), |at| {
                links.get(at).is_some_and(|link| self.held().card(id, at, link).is_some())
            }),
        };
        let Some(link) = links.into_iter().nth(index) else {
            return error_response(&ChatError::NotFound);
        };
        let (kind, decided) = decided(&self.privacy(), &link, Some(&server));
        let page = decided != Decided::Other(Decision::Skip);
        let card = match kind {
            LinkKind::Web if !page => None,
            LinkKind::Web => match web::resolve(&remote, &link).await {
                Ok(card) => Some(card),
                Err(err) => return file_refusal(&err),
            },
            _ => match self.resolve_for_app(account, &remote, &link, &kind, page).await {
                Some(card) => Some(card),
                None if matches!(kind, LinkKind::Github { .. }) && !page => None,
                None => return error_response(&ChatError::Offline),
            },
        };
        let Some(card) = card else {
            return refused("unsupported", tr("Content from this provider is turned off in Settings › Privacy."));
        };
        let mut held = self.held();
        held.keep_card(id, index, &link, card);
        Response::ChatCard { card: held.card(id, index, &link).and_then(|held| held.card.clone()) }
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
    fn previews_stay_within_their_budget() {
        let mut held = Held::default();
        let picture = |n: usize| Picture { bytes: vec![0; n], content_type: "image/png".into() };
        let half = (PREVIEWS_PER_ACCOUNT / 2) as usize;
        held.keep_preview("a", "1".into(), picture(half));
        held.keep_preview("a", "2".into(), picture(half));
        held.keep_preview("a", "3".into(), picture(10));
        assert!(held.preview("a", "1").is_none() && held.preview("a", "3").is_some());
        held.keep_card("a/tok/1", 0, "https://example.org/", Resolved { card: None, picture: None });
        held.forget_account("a");
        assert!(held.preview("a", "3").is_none() && held.cards.is_empty());
    }

    /// Issue #785: the first three links of a message get a card each, by their index; the fourth gets none, and
    /// `chat.show_card` and the picture take the index.
    #[tokio::test]
    async fn three_links_give_three_cards_and_the_fourth_none() {
        use torro_chat::{conversation_id, message_id};
        use torro_ipc::Request;

        const ACCOUNT: &str = "nextcloud:anna@cloud.example.org";
        const APP: Caller<'static> = Caller { reach: Reach::Suite, client: None };
        let server = Arc::new(super::super::fake::Server::default());
        let chat = Arc::new(ChatService::fixed(vec![(ACCOUNT.into(), Arc::clone(&server))]));
        let always = torro_core::privacy::Privacy {
            content: torro_core::privacy::ForeignContent::Always,
            ..torro_core::privacy::Privacy::default()
        };
        chat.set_privacy(Arc::new(move || always.clone()));
        let links = ["https://a.example.org/1", "https://b.example.org/2", "https://c.example.org/3"];
        let fourth = "https://d.example.org/4";
        {
            let mut state = server.state();
            for (n, link) in links.iter().chain([&fourth]).enumerate() {
                let reference = torro_nextcloud::Reference {
                    title: format!("Page {n}"),
                    description: None,
                    image: Some(format!("p{n}")),
                };
                state.references.insert((*link).into(), reference);
            }
            let mut png = Vec::new();
            image::DynamicImage::new_rgb8(4, 3)
                .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
                .expect("a PNG");
            state.card_pictures.insert("p2".into(), Picture { bytes: png, content_type: "image/png".into() });
            let text = format!("{} {} and {}, then {fourth}", links[0], links[1], links[2]);
            let list = state.messages.get_mut("ben4chat").expect("Ben");
            list.push(super::super::remote::tests::message(9, &text));
        }
        let ben = conversation_id(ACCOUNT, "ben4chat");
        let read = Request::ChatGetMessages {
            conversation: ben.clone(),
            before: None,
            after: None,
            around: None,
            limit: None,
        };
        let Response::ChatMessages(page) = chat.handle(read, APP).await else { panic!("messages") };
        let message = page.messages.iter().find(|message| message.number() == 9).expect("the message");
        let urls: Vec<&str> = message.cards.iter().map(|card| card.url.as_str()).collect();
        assert_eq!(urls, links);
        let indexes: Vec<u8> = message.cards.iter().map(|card| card.index).collect();
        assert_eq!(indexes, [0, 1, 2], "each card says where its link stands (S1)");
        assert_eq!(server.count("resolve"), 3, "the fourth is not resolved");

        let id = message_id(&ben, 9);
        let small = PreviewSize::Small;
        assert!(matches!(chat.card_preview(&id, 2, small, APP).await, Response::ChatPicture { .. }), "the third's");
        let code = |response: Response| match response {
            Response::Error { code, .. } => code,
            other => format!("{other:?}"),
        };
        assert_eq!(code(chat.card_preview(&id, 3, small, APP).await), "not-found");
        assert_eq!(code(chat.show_card_at(&id, Some(3)).await), "not-found", "no card for the fourth");
        let Response::ChatCard { card: Some(card) } = chat.show_card_at(&id, Some(1)).await else { panic!("a card") };
        assert_eq!((card.url.as_str(), card.index), (links[1], 1));
    }

    /// S1 and S2 of issue #785: a card carries where its link stands, also when a link before it has none; "Show
    /// once" without `index` shows the link the message asks about, not the first.
    #[tokio::test]
    async fn show_once_without_index_takes_the_link_the_message_asks_about() {
        use torro_chat::{conversation_id, message_id};
        use torro_ipc::Request;

        const ACCOUNT: &str = "nextcloud:anna@cloud.example.org";
        const APP: Caller<'static> = Caller { reach: Reach::Suite, client: None };
        let server = Arc::new(super::super::fake::Server::default());
        let chat = Arc::new(ChatService::fixed(vec![(ACCOUNT.into(), Arc::clone(&server))]));
        // The own server's page has no card; the other server's asks first (Settings › Privacy at "Ask").
        let own = "https://cloud.example.org/apps/deck/board/4";
        let page = "https://example.com/blog/post";
        {
            let mut state = server.state();
            let reference = torro_nextcloud::Reference { title: "A post".into(), description: None, image: None };
            state.references.insert(page.into(), reference);
            let list = state.messages.get_mut("ben4chat").expect("Ben");
            list.push(super::super::remote::tests::message(9, &format!("The board {own}, and {page}")));
        }
        let ben = conversation_id(ACCOUNT, "ben4chat");
        let read = Request::ChatGetMessages {
            conversation: ben.clone(),
            before: None,
            after: None,
            around: None,
            limit: None,
        };
        let Response::ChatMessages(page_of) = chat.handle(read.clone(), APP).await else { panic!("messages") };
        let message = page_of.messages.iter().find(|message| message.number() == 9).expect("the message");
        assert!(message.cards.is_empty() && message.foreign.is_some(), "{message:?}");
        let id = message_id(&ben, 9);
        let Response::ChatCard { card: Some(card) } = chat.show_card(&id).await else {
            panic!("the asked link's card")
        };
        assert_eq!((card.url.as_str(), card.index), (page, 1));
        let Response::ChatMessages(page_of) = chat.handle(read, APP).await else { panic!("messages") };
        let message = page_of.messages.iter().find(|message| message.number() == 9).expect("the message");
        let shown: Vec<(&str, u8)> = message.cards.iter().map(|card| (card.url.as_str(), card.index)).collect();
        assert_eq!(shown, [(page, 1)], "the second link's card, by its place in the text");
    }

    /// Issue #784: a card held for the person goes to a client only with the right its kind asks for: a file with the
    /// account's chat, an issue with its repository, a quote with its conversation; a page while Settings › Privacy
    /// does not turn it off.
    #[test]
    fn a_client_gets_a_held_card_only_with_the_right_of_its_kind() {
        use torro_chat::{LinkCard, LinkCardKind, LinkGithub};
        use torro_ipc::api::{Area, RightsTemplate, Selection};

        const ACCOUNT: &str = "nextcloud:anna@cloud.example.org";
        let load = Decided::Other(Decision::Load);
        let skip = Decided::Other(Decision::Skip);
        let page = LinkCard { url: "https://example.org/a".into(), ..LinkCard::default() };
        let file = LinkCard { kind: LinkCardKind::NextcloudFile, ..LinkCard::default() };
        let github = |repo: &str| LinkCard {
            kind: LinkCardKind::Github,
            github: Some(Box::new(LinkGithub {
                repo: repo.into(),
                number: 1,
                is_pull: false,
                state: torro_chat::GithubState::Open,
            })),
            ..LinkCard::default()
        };
        let quote = LinkCard { kind: LinkCardKind::ChatMessage, ..LinkCard::default() };
        let quoted = LinkKind::ChatMessage { token: "ben4chat".into(), message: 5 };
        let mut reading = RightsTemplate::ReadOnly.rights();
        reading.chat_accounts = Selection::Only(vec![ACCOUNT.into()]);
        reading.github_repos = Selection::All;
        let sees = |card: &LinkCard, kind: &LinkKind, rights: &torro_ipc::api::Rights| {
            reader_sees(card, kind, &Decided::Own, Reach::Client(rights), ACCOUNT)
        };

        assert!(reader_sees(&page, &LinkKind::Web, &load, Reach::Client(&reading), ACCOUNT));
        assert!(!reader_sees(&page, &LinkKind::Web, &skip, Reach::Suite, ACCOUNT), "turned off for everyone");
        assert!(sees(&file, &LinkKind::Web, &reading) && sees(&github("torro/workplace"), &LinkKind::Web, &reading));
        assert!(sees(&quote, &quoted, &reading));
        assert!(!sees(&quote, &LinkKind::Web, &reading), "a quote is only one of a chat message");

        let mut other_account = reading.clone();
        other_account.chat_accounts = Selection::Only(vec!["nextcloud:ben@cloud.example.org".into()]);
        assert!(!sees(&file, &LinkKind::Web, &other_account) && !sees(&quote, &quoted, &other_account));
        let mut switched_off = reading.clone();
        switched_off.denied.insert("chat.get_messages".into());
        assert!(!sees(&file, &LinkKind::Web, &switched_off), "a switched-off command counts");
        assert!(!sees(&quote, &quoted, &switched_off));
        let mut no_chat = reading.clone();
        no_chat.areas.remove(&Area::Chat);
        assert!(!sees(&file, &LinkKind::Web, &no_chat) && !sees(&quote, &quoted, &no_chat));

        let mut other_repo = reading.clone();
        other_repo.github_repos = Selection::Only(vec!["someone/else".into()]);
        assert!(!sees(&github("torro/workplace"), &LinkKind::Web, &other_repo));
        let mut no_github = reading.clone();
        no_github.areas.remove(&Area::Github);
        assert!(!sees(&github("torro/workplace"), &LinkKind::Web, &no_github));
        for card in [&file, &github("torro/workplace"), &quote] {
            assert!(reader_sees(card, &quoted, &Decided::Own, Reach::Suite, ACCOUNT), "the person sees every kind");
        }
    }

    /// Review of #839: a message edited from a quote of one conversation (Team Alpha) to a link on another (Ben's)
    /// keeps no card of the first under the new link. A client that reads the chat, checked for the new link, gets
    /// no text of Team Alpha; the app's next read resolves the new link. (Rights reach a whole chat account, so the
    /// client could read both; what matters is that the right checked and the card shown are of the same link.)
    #[tokio::test]
    async fn an_edited_link_shows_no_card_of_the_link_before() {
        use torro_chat::{conversation_id, message_id};
        use torro_ipc::Request;
        use torro_ipc::api::{RightsTemplate, Selection};

        const ACCOUNT: &str = "nextcloud:anna@cloud.example.org";
        const APP: Caller<'static> = Caller { reach: Reach::Suite, client: None };
        let server = Arc::new(super::super::fake::Server::default());
        let chat = Arc::new(ChatService::fixed(vec![(ACCOUNT.into(), Arc::clone(&server))]));
        let link = |token: &str, number: u64| format!("{}/call/{token}#message_{number}", super::super::fake::SERVER);
        let (team, ben_three) = (link("team4alpha", 1), link("ben4chat", 3));
        {
            let mut state = server.state();
            let mut own = super::super::remote::tests::message(9, &format!("see {team}"));
            own.author = Some(super::super::fake::ME.into());
            state.messages.get_mut("ben4chat").expect("Ben").push(own);
        }
        let ben = conversation_id(ACCOUNT, "ben4chat");
        let read = Request::ChatGetMessages {
            conversation: ben.clone(),
            before: None,
            after: None,
            around: None,
            limit: None,
        };
        let cards = |response: Response| match response {
            Response::ChatMessages(page) => {
                page.messages.into_iter().find(|message| message.number() == 9).map(|m| m.cards).unwrap_or_default()
            }
            other => panic!("messages: {other:?}"),
        };
        let quoted = |cards: &[torro_chat::LinkCard]| -> Vec<String> {
            cards.iter().filter_map(|card| Some(card.quote.as_ref()?.text.clone())).collect()
        };
        assert_eq!(quoted(&cards(chat.handle(read.clone(), APP).await)), ["hello team"], "the quote of Team Alpha");

        let edit = Request::ChatEditMessage { id: message_id(&ben, 9), text: format!("see {ben_three}") };
        assert_eq!(chat.handle(edit, APP).await, Response::Ok);
        let mut rights = RightsTemplate::ReadOnly.rights();
        rights.chat_accounts = Selection::Only(vec![ACCOUNT.into()]);
        let agent = Caller { reach: Reach::Client(&rights), client: Some("agent") };
        let theirs = cards(chat.handle(read.clone(), agent).await);
        assert!(theirs.iter().all(|card| card.url != team), "no card of the link before: {theirs:?}");
        assert!(quoted(&theirs).is_empty(), "no text of Team Alpha: {theirs:?}");

        assert_eq!(quoted(&cards(chat.handle(read.clone(), APP).await)), ["three"], "the new link resolved");
        assert_eq!(quoted(&cards(chat.handle(read, agent).await)), ["three"], "and held for the client too");
    }

    /// Issue #784: a GitHub link no account shows is held, so GitHub is not asked with every read of the messages.
    #[tokio::test]
    async fn a_github_link_without_an_item_is_not_asked_with_every_read() {
        use crate::account::github::tests::{MemoryTokens, NO_CLI, fake_github, service, viewer};
        use torro_chat::conversation_id;
        use torro_ipc::Request;

        const ACCOUNT: &str = "nextcloud:anna@cloud.example.org";
        const APP: Caller<'static> = Caller { reach: Reach::Suite, client: None };
        let server = Arc::new(super::super::fake::Server::default());
        let chat = Arc::new(ChatService::fixed(vec![(ACCOUNT.into(), Arc::clone(&server))]));
        let (url, record) = fake_github(|seen| match seen.body.contains("viewer") {
            true => (200, Some("repo"), viewer("anna-berg")),
            false => (200, None, r#"{"data":{"i0":{"issueOrPullRequest":null}}}"#.into()),
        })
        .await;
        let dir = tempfile::tempdir().expect("temp dir");
        let github = service(dir.path(), &MemoryTokens::default(), &url, NO_CLI);
        github.connect_token(torro_core::account::Secret::new("ghp_testTOKEN")).await.expect("connected");
        record.lock().expect("record").clear();
        chat.set_github(Arc::new(github));
        {
            let mut state = server.state();
            let list = state.messages.get_mut("ben4chat").expect("Ben");
            list.push(super::super::remote::tests::message(9, "See https://github.com/torro/workplace/issues/9"));
        }
        let read = Request::ChatGetMessages {
            conversation: conversation_id(ACCOUNT, "ben4chat"),
            before: None,
            after: None,
            around: None,
            limit: None,
        };
        for _ in 0..3 {
            let Response::ChatMessages(page) = chat.handle(read.clone(), APP).await else { panic!("messages") };
            let message = page.messages.iter().find(|message| message.number() == 9).expect("the message");
            assert!(message.cards.is_empty() && message.foreign.is_some(), "the page asks: {message:?}");
        }
        assert_eq!(record.lock().expect("record").len(), 1, "GitHub asked once");
    }

    /// Issue #790: a link to a file of the own server gets its file card for the app, with the server's preview as
    /// its picture; a client that reads the chat of the account gets both too (issue #784), one without the right
    /// to picture content not the picture.
    #[tokio::test]
    async fn a_file_of_the_own_server_goes_to_readers_of_its_chat() {
        use torro_chat::{LinkCardKind, conversation_id, message_id};
        use torro_ipc::Request;
        use torro_ipc::api::RightsTemplate;

        const ACCOUNT: &str = "nextcloud:anna@cloud.example.org";
        const APP: Caller<'static> = Caller { reach: Reach::Suite, client: None };
        let server = Arc::new(super::super::fake::Server::default());
        let chat = Arc::new(ChatService::fixed(vec![(ACCOUNT.into(), Arc::clone(&server))]));
        let file = "https://cloud.example.org/f/12";
        {
            let mut state = server.state();
            let reference = torro_nextcloud::Reference { title: "Plan.pdf".into(), description: None, image: None };
            state.references.insert(file.into(), reference);
            let mut png = Vec::new();
            image::DynamicImage::new_rgb8(4, 3)
                .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
                .expect("a PNG");
            state.previews.insert(12, Picture { bytes: png, content_type: "image/png".into() });
            let list = state.messages.get_mut("ben4chat").expect("Ben");
            list.push(super::super::remote::tests::message(9, &format!("the plan: {file}")));
        }
        let ben = conversation_id(ACCOUNT, "ben4chat");
        let read = Request::ChatGetMessages {
            conversation: ben.clone(),
            before: None,
            after: None,
            around: None,
            limit: None,
        };
        let card = |response: Response| match response {
            Response::ChatMessages(page) => {
                page.messages.into_iter().find(|message| message.number() == 9).and_then(|m| m.cards.first().cloned())
            }
            other => panic!("messages: {other:?}"),
        };
        let shown = card(chat.handle(read.clone(), APP).await).expect("a file card");
        assert_eq!((shown.kind, shown.title.as_str(), shown.image), (LinkCardKind::NextcloudFile, "Plan.pdf", true));
        let id = message_id(&ben, 9);
        let small = PreviewSize::Small;
        assert!(matches!(chat.card_preview(&id, 0, small, APP).await, Response::ChatPicture { .. }));

        let mut rights = RightsTemplate::FullAccess.rights();
        rights.image_content = true;
        rights.chat_accounts = torro_ipc::api::Selection::Only(vec![ACCOUNT.into()]);
        let agent = Caller { reach: Reach::Client(&rights), client: Some("agent") };
        let theirs = card(chat.handle(read, agent).await).expect("a reader of the chat gets the card");
        assert_eq!(theirs.kind, LinkCardKind::NextcloudFile);
        assert!(matches!(chat.card_preview(&id, 0, small, agent).await, Response::ChatPicture { .. }));
        rights.image_content = false;
        let blind = Caller { reach: Reach::Client(&rights), client: Some("agent") };
        assert!(matches!(chat.card_preview(&id, 0, small, blind).await, Response::Error { .. }), "no picture right");
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
