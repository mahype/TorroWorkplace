//! A chat server in memory for the tests of the chat and presence services: it answers what
//! [`super::remote::Remote`] asks, notes every call by name, and can be switched offline.

use std::collections::BTreeMap;
use std::sync::{Mutex, MutexGuard};
use std::time::Duration;

use chrono::{DateTime, Utc};
use torro_chat::{ChatError, ConversationKind, Mention, MessageKind, NotifyLevel, Preview, Status};

use super::remote::tests::message;
use super::remote::{Caps, RemoteHit, RemoteMessage, RemotePerson, RemoteQuote, RemoteRoom, RemoteStatus, Sending};

/// The user id of the fake's own person.
pub const ME: &str = "anna";

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
