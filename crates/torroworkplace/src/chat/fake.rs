//! A chat server in memory for the tests of the chat and presence services: it answers what
//! [`super::remote::Remote`] asks, notes every call by name, and can be switched offline. Files
//! (stage 4) live in [`State::files`] by path; sharing one posts a message with it.

use std::collections::BTreeMap;
use std::sync::{Mutex, MutexGuard};
use std::time::Duration;

use chrono::{DateTime, Utc};
use torro_chat::{ChatError, ConversationKind, FileRef, Mention, MessageKind, NotifyLevel, Preview, Status};

use super::remote::tests::message;
use super::remote::{
    CallPeople, Caps, DavPath, FileError, Picture, Reference, RemoteHit, RemoteMessage, RemotePerson, RemoteQuote,
    RemoteRing, RemoteRoom, RemoteStatus, Sending, ShareMeta, Uploaded,
};

/// The user id of the fake's own person.
pub const ME: &str = "anna";

/// The fake's address.
pub const SERVER: &str = "https://cloud.example.org";

/// A file in the fake's storage.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredFile {
    pub id: u64,
    pub bytes: Vec<u8>,
    pub etag: String,
}

#[derive(Debug)]
pub struct State {
    pub caps: Caps,
    pub rooms: Vec<RemoteRoom>,
    pub messages: BTreeMap<String, Vec<RemoteMessage>>,
    pub people: Vec<RemotePerson>,
    pub statuses: Vec<RemoteStatus>,
    pub own: RemoteStatus,
    /// Every call answers `offline`.
    pub offline: bool,
    /// The names of the calls made, in order ("rooms", "messages tok1", "send tok1" …).
    pub calls: Vec<String>,
    /// The sends as they came in, by token.
    pub sent: Vec<(String, Sending)>,
    /// The heartbeats sent.
    pub beats: Vec<Status>,
    /// The person's files, by path (`/Talk/x.png`).
    pub files: BTreeMap<String, StoredFile>,
    /// The pieces of chunked uploads, by upload id.
    pub chunks: BTreeMap<String, Vec<(u32, Vec<u8>)>>,
    /// The shares as they came in: token, path, meta data.
    pub shares: Vec<(String, String, ShareMeta)>,
    /// Sharing fails (the server refuses).
    pub refuse_shares: bool,
    /// The cards the server knows, by link.
    pub references: BTreeMap<String, Reference>,
    /// The previews the server draws, by file id.
    pub previews: BTreeMap<u64, Picture>,
    /// The pictures of cards, by id.
    pub card_pictures: BTreeMap<String, Picture>,
    /// Free bytes of the person's storage; `None` is unlimited.
    pub free: Option<u64>,
    /// The calls that ring for the person, as notifications (stage 6).
    pub rings: Vec<RemoteRing>,
    /// The user ids of the people in the call of a conversation, by token.
    pub in_calls: BTreeMap<String, Vec<String>>,
}

/// See the module docs.
#[derive(Debug)]
pub struct Server {
    state: Mutex<State>,
}

/// A direct chat with `user`, read up to nothing, with `unread` unread messages.
pub fn room(token: &str, name: &str, kind: ConversationKind, unread: u64) -> RemoteRoom {
    RemoteRoom {
        token: token.into(),
        name: name.into(),
        kind,
        unread,
        mentioned: false,
        mentioned_directly: false,
        last_message: None,
        last_number: None,
        last_activity: DateTime::from_timestamp(1_800_000_000, 0).unwrap_or_default(),
        last_read: None,
        other_user: None,
        has_call: false,
        call_since: None,
        in_call: false,
        favorite: false,
        read_only: false,
        federated: false,
        notify: NotifyLevel::Default,
    }
}

fn status(user: &str, status: Status, message: Option<&str>) -> RemoteStatus {
    RemoteStatus { user: user.into(), status, message: message.map(str::to_string), icon: None, clear_at: None }
}

impl Default for Server {
    /// Anna's server: a direct chat with Ben (`ben4chat`, five messages, two unread), the group
    /// "Team Alpha" (`team4alpha`), notes to herself (`note4self`) and Talk's own news; Ben online,
    /// Cem away, Dana without a status.
    fn default() -> Server {
        let caps = Caps {
            chat: true,
            read_marker: true,
            unread: true,
            keep_notifications: true,
            max_length: 1000,
            silent: true,
            reactions: true,
            edit: true,
            delete: true,
            search: true,
            status: true,
            attachments: true,
            attachments_folder: "/Talk".into(),
            subfolders: false,
            chunk_max: None,
            caption: true,
            shared: true,
            reference_ids: true,
            context: true,
        };
        let mut ben = room("ben4chat", "Ben Koch", ConversationKind::Direct, 2);
        ben.other_user = Some("ben".into());
        ben.last_number = Some(5);
        ben.last_read = Some(3);
        ben.last_message = Some(Preview { author: "Ben Koch".into(), text: "five".into(), system: false });
        let team = room("team4alpha", "Team Alpha", ConversationKind::Group, 0);
        let note = room("note4self", "Note to self", ConversationKind::NoteToSelf, 0);
        let news = room("talk4news", "Talk updates", ConversationKind::Other, 1);
        let mut own_message = message(2, "two");
        own_message.author = Some(ME.into());
        own_message.author_name = "Anna Berg".into();
        let ben_messages =
            vec![message(1, "one"), own_message, message(3, "three"), message(4, "four"), message(5, "five")];
        let people = vec![
            RemotePerson { id: "ben".into(), name: "Ben Koch".into() },
            RemotePerson { id: "cem".into(), name: "Cem Demir".into() },
            RemotePerson { id: "dana".into(), name: "Dana Erb".into() },
        ];
        Server {
            state: Mutex::new(State {
                caps,
                rooms: vec![ben, team, note, news],
                messages: BTreeMap::from([
                    ("ben4chat".to_string(), ben_messages),
                    ("team4alpha".to_string(), vec![message(1, "hello team")]),
                    ("note4self".to_string(), Vec::new()),
                ]),
                people,
                statuses: vec![status("ben", Status::Online, Some("In the office")), status("cem", Status::Away, None)],
                own: status(ME, Status::Online, None),
                offline: false,
                calls: Vec::new(),
                sent: Vec::new(),
                beats: Vec::new(),
                files: BTreeMap::new(),
                chunks: BTreeMap::new(),
                shares: Vec::new(),
                refuse_shares: false,
                references: BTreeMap::new(),
                previews: BTreeMap::new(),
                card_pictures: BTreeMap::new(),
                free: None,
                rings: Vec::new(),
                in_calls: BTreeMap::new(),
            }),
        }
    }
}

impl Server {
    /// The server's state, to arrange a test or look at what was asked.
    pub fn state(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Notes the call `name` and refuses it while the server is offline.
    fn call(&self, name: impl Into<String>) -> Result<MutexGuard<'_, State>, ChatError> {
        let mut state = self.state();
        state.calls.push(name.into());
        if state.offline { Err(ChatError::Offline) } else { Ok(state) }
    }

    /// How often a call whose name starts with `name` was made.
    pub fn count(&self, name: &str) -> usize {
        self.state().calls.iter().filter(|call| call.starts_with(name)).count()
    }

    pub async fn user(&self) -> Result<RemotePerson, ChatError> {
        self.call("user").map(|_| RemotePerson { id: ME.into(), name: "Anna Berg".into() })
    }

    pub async fn caps(&self) -> Result<Caps, ChatError> {
        self.call("caps").map(|state| state.caps.clone())
    }

    pub async fn rooms(&self) -> Result<Vec<RemoteRoom>, ChatError> {
        self.call("rooms").map(|state| state.rooms.clone())
    }

    pub async fn messages(
        &self,
        token: &str,
        before: Option<u64>,
        limit: u32,
        _caps: &Caps,
    ) -> Result<Vec<RemoteMessage>, ChatError> {
        let state = self.call(format!("messages {token}"))?;
        let all = state.messages.get(token).ok_or(ChatError::NotFound)?;
        let older: Vec<&RemoteMessage> = all.iter().filter(|m| before.is_none_or(|before| m.number < before)).collect();
        let from = older.len().saturating_sub(limit as usize);
        Ok(older[from..].iter().map(|m| (*m).clone()).collect())
    }

    pub async fn wait(
        &self,
        token: &str,
        after: u64,
        _seconds: u32,
        _caps: &Caps,
    ) -> Result<Vec<RemoteMessage>, ChatError> {
        let new: Vec<RemoteMessage> = {
            let state = self.call(format!("wait {token}"))?;
            let all = state.messages.get(token).ok_or(ChatError::NotFound)?;
            all.iter().filter(|m| m.number > after).cloned().collect()
        };
        if new.is_empty() {
            // A real server holds the request open; the tests only need it to give way.
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
        Ok(new)
    }

    pub async fn send(&self, token: &str, sending: &Sending) -> Result<RemoteMessage, ChatError> {
        let mut state = self.call(format!("send {token}"))?;
        state.sent.push((token.to_string(), sending.clone()));
        let list = state.messages.get_mut(token).ok_or(ChatError::NotFound)?;
        let number = list.last().map_or(1, |last| last.number + 1);
        let mut sent = message(number, &sending.text);
        sent.author = Some(ME.into());
        sent.author_name = "Anna Berg".into();
        sent.reply_to = sending.reply_to.and_then(|quoted| list.iter().find(|m| m.number == quoted)).map(|quoted| {
            RemoteQuote { number: quoted.number, author: quoted.author_name.clone(), text: quoted.text.clone() }
        });
        list.push(sent.clone());
        Ok(sent)
    }

    pub async fn mark_read(&self, token: &str, up_to: u64) -> Result<(), ChatError> {
        let mut state = self.call(format!("mark_read {token} {up_to}"))?;
        let room = state.rooms.iter_mut().find(|room| room.token == token).ok_or(ChatError::NotFound)?;
        room.last_read = Some(up_to);
        room.unread = 0;
        Ok(())
    }

    pub async fn mark_unread(&self, token: &str) -> Result<(), ChatError> {
        let mut state = self.call(format!("mark_unread {token}"))?;
        let room = state.rooms.iter_mut().find(|room| room.token == token).ok_or(ChatError::NotFound)?;
        room.unread = 1;
        Ok(())
    }

    pub async fn mentions(&self, token: &str, query: &str, limit: u32) -> Result<Vec<Mention>, ChatError> {
        let state = self.call(format!("mentions {token}"))?;
        let query = query.to_lowercase();
        Ok(state
            .people
            .iter()
            .filter(|person| person.name.to_lowercase().contains(&query))
            .take(limit as usize)
            .map(|person| Mention { id: person.id.clone(), name: person.name.clone() })
            .collect())
    }

    pub async fn find_people(&self, query: &str, limit: u32) -> Result<Vec<RemotePerson>, ChatError> {
        let state = self.call("find_people")?;
        let query = query.to_lowercase();
        Ok(state
            .people
            .iter()
            .filter(|person| person.name.to_lowercase().contains(&query))
            .take(limit as usize)
            .cloned()
            .collect())
    }

    pub async fn find_users(&self, query: &str, limit: u32) -> Result<Vec<RemotePerson>, ChatError> {
        self.find_people(query, limit).await
    }

    pub async fn create_direct(&self, user: &str) -> Result<RemoteRoom, ChatError> {
        let mut state = self.call(format!("create_direct {user}"))?;
        if let Some(existing) = state.rooms.iter().find(|room| room.other_user.as_deref() == Some(user)) {
            return Ok(existing.clone());
        }
        let name = state.people.iter().find(|person| person.id == user).ok_or(ChatError::NotFound)?.name.clone();
        let token = format!("{user}4chat");
        let mut created = room(&token, &name, ConversationKind::Direct, 0);
        created.other_user = Some(user.to_string());
        state.rooms.push(created.clone());
        state.messages.insert(token, Vec::new());
        Ok(created)
    }

    pub async fn edit(&self, token: &str, number: u64, text: &str) -> Result<(), ChatError> {
        let mut state = self.call(format!("edit {token} {number}"))?;
        let list = state.messages.get_mut(token).ok_or(ChatError::NotFound)?;
        let target = list.iter_mut().find(|m| m.number == number).ok_or(ChatError::NotFound)?;
        target.text = text.to_string();
        target.edited = true;
        Ok(())
    }

    pub async fn delete(&self, token: &str, number: u64) -> Result<(), ChatError> {
        let mut state = self.call(format!("delete {token} {number}"))?;
        let list = state.messages.get_mut(token).ok_or(ChatError::NotFound)?;
        let target = list.iter_mut().find(|m| m.number == number).ok_or(ChatError::NotFound)?;
        target.kind = MessageKind::Deleted;
        target.text.clear();
        Ok(())
    }

    pub async fn react(&self, token: &str, number: u64, emoji: &str, remove: bool) -> Result<(), ChatError> {
        self.call(format!("react {token} {number} {emoji} {remove}")).map(|_| ())
    }

    pub async fn set_favorite(&self, token: &str, favorite: bool) -> Result<(), ChatError> {
        let mut state = self.call(format!("set_favorite {token} {favorite}"))?;
        let room = state.rooms.iter_mut().find(|room| room.token == token).ok_or(ChatError::NotFound)?;
        room.favorite = favorite;
        Ok(())
    }

    pub async fn set_notify(&self, token: &str, level: NotifyLevel) -> Result<(), ChatError> {
        let mut state = self.call(format!("set_notify {token}"))?;
        let room = state.rooms.iter_mut().find(|room| room.token == token).ok_or(ChatError::NotFound)?;
        room.notify = level;
        Ok(())
    }

    pub async fn search(&self, query: &str, limit: u32) -> Result<Vec<RemoteHit>, ChatError> {
        let state = self.call("search")?;
        let query = query.to_lowercase();
        Ok(state
            .messages
            .iter()
            .flat_map(|(token, list)| list.iter().map(move |m| (token, m)))
            .filter(|(_, m)| m.text.to_lowercase().contains(&query))
            .take(limit as usize)
            .map(|(token, m)| RemoteHit {
                token: token.clone(),
                number: m.number,
                title: m.author_name.clone(),
                text: m.text.clone(),
            })
            .collect())
    }

    pub async fn statuses(&self) -> Result<Vec<RemoteStatus>, ChatError> {
        self.call("statuses").map(|state| state.statuses.clone())
    }

    pub async fn status_of(&self, user: &str) -> Result<Option<RemoteStatus>, ChatError> {
        self.call(format!("status_of {user}")).map(|state| state.statuses.iter().find(|s| s.user == user).cloned())
    }

    pub async fn own_status(&self) -> Result<RemoteStatus, ChatError> {
        self.call("own_status").map(|state| state.own.clone())
    }

    pub async fn set_status(&self, status: Status) -> Result<(), ChatError> {
        self.call(format!("set_status {}", status.as_str())).map(|mut state| state.own.status = status)
    }

    pub async fn set_status_message(
        &self,
        message: Option<&str>,
        icon: Option<&str>,
        clear_at: Option<DateTime<Utc>>,
    ) -> Result<(), ChatError> {
        let mut state = self.call("set_status_message")?;
        state.own.message = message.map(str::to_string);
        state.own.icon = icon.map(str::to_string);
        state.own.clear_at = clear_at;
        Ok(())
    }

    pub async fn heartbeat(&self, status: Status) -> Result<(), ChatError> {
        self.call("heartbeat").map(|mut state| state.beats.push(status))
    }
}

impl Server {
    /// Puts `bytes` as the file `path` in a message of Ben in `token`, as a server would hold a
    /// file someone shared: the message with the file, its path and the file stored.
    pub fn share_from_ben(&self, token: &str, path: &str, mimetype: &str, bytes: &[u8]) -> u64 {
        let mut state = self.state();
        let id = 9000 + state.files.len() as u64;
        state.files.insert(path.to_string(), StoredFile { id, bytes: bytes.to_vec(), etag: format!("\"e{id}\"") });
        let name = path.rsplit('/').next().unwrap_or(path).to_string();
        let list = state.messages.entry(token.to_string()).or_default();
        let number = list.last().map_or(1, |last| last.number + 1);
        let mut shared = message(number, &name);
        shared.files = vec![FileRef {
            id: id.to_string(),
            name,
            size: bytes.len() as u64,
            content_type: mimetype.to_string(),
            preview: mimetype.starts_with("image/"),
            hide_download: false,
            width: None,
            height: None,
            blurhash: Some("LEHV6nWB2yk8".into()),
        }];
        shared.paths.insert(id.to_string(), path.trim_start_matches('/').to_string());
        list.push(shared);
        number
    }

    pub async fn free_space(&self) -> Result<Option<u64>, ChatError> {
        self.call("free_space").map(|state| state.free)
    }

    pub async fn around(&self, token: &str, number: u64, limit: u32) -> Result<Vec<RemoteMessage>, ChatError> {
        let state = self.call(format!("around {token} {number}"))?;
        let all = state.messages.get(token).ok_or(ChatError::NotFound)?;
        let at = all.iter().position(|m| m.number == number).ok_or(ChatError::NotFound)?;
        let from = at.saturating_sub(limit as usize);
        let to = (at + 1 + limit as usize).min(all.len());
        Ok(all[from..to].to_vec())
    }

    pub async fn free_name(&self, _user: &str, folder: &DavPath, name: &str) -> Result<String, FileError> {
        let state = self.call("free_name")?;
        for copy in 1..=99 {
            let candidate = torro_nextcloud::numbered(name, copy);
            let path = folder.join(&candidate).ok_or(FileError::Chat(ChatError::NotFound))?;
            if !state.files.contains_key(&path.as_path()) {
                return Ok(candidate);
            }
        }
        Err(FileError::Exists)
    }

    pub async fn make_folders(&self, _user: &str, folder: &DavPath) -> Result<(), FileError> {
        let _held = self.call(format!("make_folders {}", folder.as_path()))?;
        Ok(())
    }

    /// Stores `bytes` at `path` as a new file, when there is none and the storage has room.
    fn store(state: &mut State, path: &DavPath, bytes: Vec<u8>) -> Result<Uploaded, FileError> {
        let key = path.as_path();
        if state.files.contains_key(&key) {
            return Err(FileError::Exists);
        }
        if state.free.is_some_and(|free| free < bytes.len() as u64) {
            return Err(FileError::NoSpace);
        }
        let id = 7000 + state.files.len() as u64;
        let etag = format!("\"e{id}\"");
        state.files.insert(key, StoredFile { id, bytes, etag: etag.clone() });
        Ok(Uploaded { file_id: Some(id), etag: Some(etag) })
    }

    pub async fn put_file(&self, _user: &str, path: &DavPath, bytes: Vec<u8>) -> Result<Uploaded, FileError> {
        let mut state = self.call(format!("put {}", path.as_path()))?;
        Server::store(&mut state, path, bytes)
    }

    pub async fn start_upload(
        &self,
        _user: &str,
        upload: &str,
        _target: &DavPath,
        _total: u64,
    ) -> Result<(), FileError> {
        let mut state = self.call(format!("start_upload {upload}"))?;
        state.chunks.insert(upload.to_string(), Vec::new());
        Ok(())
    }

    pub async fn put_chunk(
        &self,
        _user: &str,
        upload: &str,
        number: u32,
        bytes: Vec<u8>,
        _target: &DavPath,
        _total: u64,
    ) -> Result<(), FileError> {
        let mut state = self.call(format!("put_chunk {upload} {number}"))?;
        let pieces = state.chunks.get_mut(upload).ok_or(FileError::Chat(ChatError::NotFound))?;
        pieces.push((number, bytes));
        Ok(())
    }

    pub async fn finish_upload(
        &self,
        _user: &str,
        upload: &str,
        target: &DavPath,
        total: u64,
    ) -> Result<Uploaded, FileError> {
        let mut state = self.call(format!("finish_upload {upload}"))?;
        let mut pieces = state.chunks.remove(upload).ok_or(FileError::Chat(ChatError::NotFound))?;
        pieces.sort_by_key(|(number, _)| *number);
        let bytes: Vec<u8> = pieces.into_iter().flat_map(|(_, bytes)| bytes).collect();
        if bytes.len() as u64 != total {
            return Err(FileError::Chat(ChatError::Other("pieces missing".into())));
        }
        Server::store(&mut state, target, bytes)
    }

    pub async fn cancel_upload(&self, _user: &str, upload: &str) -> Result<(), FileError> {
        let mut state = self.call(format!("cancel_upload {upload}"))?;
        state.chunks.remove(upload);
        Ok(())
    }

    pub async fn delete_file(&self, _user: &str, path: &DavPath, etag: &str) -> Result<(), FileError> {
        let mut state = self.call(format!("delete_file {}", path.as_path()))?;
        match state.files.get(&path.as_path()) {
            Some(file) if file.etag != etag => Err(FileError::Exists),
            Some(_) => {
                state.files.remove(&path.as_path());
                Ok(())
            }
            None => Ok(()),
        }
    }

    pub async fn get_file(&self, _user: &str, path: &DavPath, max: u64) -> Result<Vec<u8>, FileError> {
        let state = self.call(format!("get_file {}", path.as_path()))?;
        let file = state.files.get(&path.as_path()).ok_or(FileError::Chat(ChatError::NotFound))?;
        if file.bytes.len() as u64 > max {
            return Err(FileError::TooLarge);
        }
        Ok(file.bytes.clone())
    }

    pub async fn preview(&self, file: u64, _edge: u32) -> Result<Picture, FileError> {
        let state = self.call(format!("preview {file}"))?;
        state.previews.get(&file).cloned().ok_or(FileError::Chat(ChatError::NotFound))
    }

    pub async fn card_picture(&self, id: &str) -> Result<Picture, FileError> {
        let state = self.call(format!("card_picture {id}"))?;
        state.card_pictures.get(id).cloned().ok_or(FileError::Chat(ChatError::NotFound))
    }

    pub async fn resolve(&self, link: &str) -> Result<Option<Reference>, FileError> {
        let state = self.call(format!("resolve {link}"))?;
        Ok(state.references.get(link).cloned())
    }

    pub async fn share(&self, token: &str, path: &DavPath, meta: &ShareMeta) -> Result<(), FileError> {
        let mut state = self.call(format!("share {token}"))?;
        if state.refuse_shares {
            return Err(FileError::Chat(ChatError::Unsupported("refused".into())));
        }
        let file = state.files.get(&path.as_path()).cloned().ok_or(FileError::Chat(ChatError::NotFound))?;
        state.shares.push((token.to_string(), path.as_path(), meta.clone()));
        let list = state.messages.get_mut(token).ok_or(FileError::Chat(ChatError::NotFound))?;
        let number = list.last().map_or(1, |last| last.number + 1);
        let mut shared = message(number, meta.caption.as_deref().unwrap_or(path.name()));
        shared.author = Some(ME.into());
        shared.author_name = "Anna Berg".into();
        shared.files = vec![FileRef {
            id: file.id.to_string(),
            name: path.name().to_string(),
            size: file.bytes.len() as u64,
            content_type: "application/octet-stream".into(),
            preview: false,
            hide_download: false,
            width: None,
            height: None,
            blurhash: None,
        }];
        shared.paths.insert(file.id.to_string(), path.as_path().trim_start_matches('/').to_string());
        list.push(shared);
        Ok(())
    }

    pub async fn shared(
        &self,
        token: &str,
        kind: &str,
        before: Option<u64>,
        limit: u32,
    ) -> Result<Vec<RemoteMessage>, ChatError> {
        let state = self.call(format!("shared {token} {kind}"))?;
        let all = state.messages.get(token).ok_or(ChatError::NotFound)?;
        let mut found: Vec<RemoteMessage> = all
            .iter()
            .filter(|m| before.is_none_or(|before| m.number < before))
            .filter(|m| m.files.iter().any(|file| kind_of(&file.content_type) == kind))
            .cloned()
            .collect();
        found.reverse();
        found.truncate(limit as usize);
        Ok(found)
    }

    pub async fn overview(&self, token: &str, limit: u32) -> Result<Vec<(String, RemoteMessage)>, ChatError> {
        let state = self.call(format!("overview {token}"))?;
        let all = state.messages.get(token).ok_or(ChatError::NotFound)?;
        let mut found = Vec::new();
        for kind in ["media", "file", "audio"] {
            let of_kind = all.iter().rev().filter(|m| m.files.iter().any(|file| kind_of(&file.content_type) == kind));
            found.extend(of_kind.take(limit as usize).map(|m| (kind.to_string(), m.clone())));
        }
        Ok(found)
    }

    pub async fn call_people(&self, token: &str) -> Result<CallPeople, ChatError> {
        let state = self.call(format!("call_people {token}"))?;
        if !state.rooms.iter().any(|room| room.token == token) {
            return Err(ChatError::NotFound);
        }
        let users = state.in_calls.get(token).cloned().unwrap_or_default();
        Ok(CallPeople { count: u32::try_from(users.len()).unwrap_or(u32::MAX), users })
    }

    pub async fn rings(&self) -> Result<Vec<RemoteRing>, ChatError> {
        self.call("rings").map(|state| state.rings.clone())
    }

    pub async fn end_ring(&self, notification: u64) -> Result<(), ChatError> {
        let mut state = self.call(format!("end_ring {notification}"))?;
        state.rings.retain(|ring| ring.notification != notification);
        Ok(())
    }
}

/// Talk's kind of a shared file of `mimetype`, as far as the fake needs it.
fn kind_of(mimetype: &str) -> &'static str {
    match mimetype.split('/').next() {
        Some("image" | "video") => "media",
        Some("audio") => "audio",
        _ => "file",
    }
}
