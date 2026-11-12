//! Calls in the chat (stage 6b of `docs/chat.md`): who rings the person, `chat.get_call`,
//! `chat.decline_call` and `chat.leave_call`. The call itself runs in the browser until the suite has
//! its own call window; the core never joins a call (a second participant without picture or sound
//! would sit in it).
//!
//! - **Ringing** is a notification of Talk about a call (decision 9), younger than [`RING_FOR`], in a
//!   conversation whose call still runs and which the person has not joined on another device (they
//!   are not among its people). While
//!   "Ring for incoming calls" is on, the core looks every [`RING_EVERY`] ([`ChatService::look_for_rings`])
//!   and opens the ringing window once per call; off, it looks at nothing.
//! - **Declining** deletes the notification on the server: it stops ringing on the person's other
//!   devices too, and the others go on calling. Only a notification that still rings: a missed call
//!   stays in the person's notifications and inbox.
//! - **Clients ask the server one at a time**, as for messages (decision 9), so no agent keeps it busy.
//! - **Nothing of the call** goes out: how many are in it, never who, and no picture or sound.

use std::sync::Arc;
use std::time::Duration;

use chrono::{DateTime, Utc};
use torro_chat::{ChatError, conversation_id};
use torro_ipc::Response;
use torro_ipc::chat::ChatCall;

use super::ChatService;
use super::remote::{Remote, RemoteRing};
use super::service::{Caller, conversation_of};

/// How often the core looks whether someone calls, while "Ring for incoming calls" is on.
pub const RING_EVERY: Duration = Duration::from_secs(10);

/// How long a call rings at most (Talk's own window gives up after 45 seconds).
pub const RING_FOR: chrono::TimeDelta = chrono::TimeDelta::seconds(45);

/// Whether `ring` is young enough at `now` to ring; a clock of the server a little ahead counts as
/// now.
fn young(ring: &RemoteRing, now: DateTime<Utc>) -> bool {
    now.signed_duration_since(ring.at) <= RING_FOR
}

impl ChatService {
    /// One look at the calls of every account with a chat at `now` (the loop of the core while "Ring
    /// for incoming calls" is on): keeps what rings for `chat.get_call` and `chat.list_conversations`,
    /// and answers the conversations that began ringing since the last look, each call once. An
    /// account that cannot be asked keeps nothing ringing.
    pub async fn look_for_rings(self: &Arc<Self>, now: DateTime<Utc>) -> Vec<String> {
        let mut began = Vec::new();
        for account in self.accounts().await {
            let ringing = match self.ready(&account).await {
                Ok((remote, _, me)) => self.ringing(&account, &remote, &me, now).await.unwrap_or_default(),
                Err(_) => Vec::new(),
            };
            let mut cache = self.cache();
            let entry = cache.account(&account);
            for ring in &ringing {
                let conversation = conversation_id(&account, &ring.token);
                if entry.announced.insert(ring.notification) && !began.contains(&conversation) {
                    began.push(conversation);
                }
            }
            entry.announced.retain(|id| ringing.iter().any(|ring| ring.notification == *id));
            entry.rings = ringing;
        }
        began
    }

    /// The calls that ring at `now` for the person `me` of `account`: the server's notifications of
    /// calls younger than [`RING_FOR`], in conversations whose call still runs and the person is not in
    /// on any device. Asks the list of conversations and who is in a call only while something rings.
    async fn ringing(
        &self,
        account: &str,
        remote: &Remote,
        me: &str,
        now: DateTime<Utc>,
    ) -> Result<Vec<RemoteRing>, ChatError> {
        let rings = self.asked(account, remote.rings().await).await?;
        let rings: Vec<RemoteRing> = rings.into_iter().filter(|ring| young(ring, now)).collect();
        if rings.is_empty() {
            return Ok(rings);
        }
        // Failing keeps the list held.
        let _ = self.refresh_rooms(account).await;
        let mut ringing = Vec::new();
        for ring in rings {
            let room = self.cache().account(account).room(&ring.token).cloned();
            let Some(room) = room.filter(|room| room.has_call && !room.in_call) else { continue };
            let people = self.asked(account, remote.call_people(&room.token).await).await.unwrap_or_default();
            if !people.users.iter().any(|user| user == me) {
                ringing.push(ring);
            }
        }
        Ok(ringing)
    }

    /// Forgets what rings ("Ring for incoming calls" switched off): nothing rings until the next look.
    pub fn stop_rings(&self) {
        for entry in self.cache().accounts.values_mut() {
            entry.rings.clear();
        }
    }

    /// Whether the call of the conversation `token` of `account` rings for the person at `now`, as the
    /// last look found it.
    pub(super) fn rings(&self, account: &str, token: &str, now: DateTime<Utc>) -> bool {
        let mut cache = self.cache();
        cache.account(account).rings.iter().any(|ring| ring.token == token && young(ring, now))
    }

    /// `chat.get_call`: whether a call runs in `id`, whether it rings, since when, how many are in it
    /// and whether the person is; the link to take part in the browser.
    pub(super) async fn get_call(self: &Arc<Self>, id: &str, caller: Caller<'_>) -> Result<Response, ChatError> {
        let (account, token) = conversation_of(id)?;
        let _one = self.one_read(caller).await;
        let (remote, _, me) = self.ready(account).await?;
        let room = self.room(account, &token).await?;
        let people = match room.has_call {
            true => self.asked(account, remote.call_people(token.as_str()).await).await?,
            false => super::remote::CallPeople::default(),
        };
        let joined = room.has_call && (room.in_call || people.users.contains(&me));
        let ringing = room.has_call && !joined && self.rings(account, token.as_str(), Utc::now());
        let call = ChatCall {
            conversation: id.to_string(),
            active: room.has_call,
            ringing,
            since: room.call_since.filter(|_| room.has_call),
            people: people.count,
            joined,
            link: format!("{}/call/{}", remote.server().trim_end_matches('/'), token.as_str()),
        };
        Ok(Response::ChatCall { call })
    }

    /// `chat.decline_call`: deletes the notifications of a call in `id` that still ring on the server,
    /// so it stops ringing on all the person's devices; the others go on. Missed calls stay. `ok` when
    /// nothing rings.
    pub(super) async fn decline_call(self: &Arc<Self>, id: &str, caller: Caller<'_>) -> Result<Response, ChatError> {
        let (account, token) = conversation_of(id)?;
        let _one = self.one_read(caller).await;
        let (remote, _, me) = self.ready(account).await?;
        self.room(account, &token).await?;
        // The server's list, not only what the last look found: a call may ring since.
        let rings = self.ringing(account, &remote, &me, Utc::now()).await?;
        for ring in rings.iter().filter(|ring| ring.token == token.as_str()) {
            self.asked(account, remote.end_ring(ring.notification).await).await?;
        }
        self.cache().account(account).rings.retain(|ring| ring.token != token.as_str());
        Ok(Response::Ok)
    }

    /// `chat.leave_call`: the person takes part in the browser, which the core cannot leave for them,
    /// so it changes nothing; `not-found` for a conversation there is none of.
    pub(super) async fn leave_call(self: &Arc<Self>, id: &str) -> Result<Response, ChatError> {
        let (account, token) = conversation_of(id)?;
        self.ready(account).await?;
        self.room(account, &token).await?;
        Ok(Response::Ok)
    }
}

#[cfg(test)]
mod tests {
    use torro_chat::conversation_id;
    use torro_ipc::Request;

    use super::super::fake::Server;
    use super::super::service::{Caller, error_response};
    use super::*;
    use crate::pim::Reach;

    const ACCOUNT: &str = "nextcloud:anna@cloud.example.org";
    const APP: Caller<'static> = Caller { reach: Reach::Suite, client: None };

    fn service() -> (Arc<ChatService>, Arc<Server>) {
        let server = Arc::new(Server::default());
        (Arc::new(ChatService::fixed(vec![(ACCOUNT.into(), Arc::clone(&server))])), server)
    }

    fn ben() -> String {
        conversation_id(ACCOUNT, "ben4chat")
    }

    /// Ben calls: his conversation has a call and Anna a notification of it, `seconds` ago.
    fn ben_calls(server: &Server, seconds: i64) {
        let mut state = server.state();
        let at = Utc::now() - chrono::TimeDelta::seconds(seconds);
        state.rings = vec![RemoteRing { token: "ben4chat".into(), notification: 41, at }];
        state.in_calls.insert("ben4chat".into(), vec!["ben".into()]);
        let room = state.rooms.iter_mut().find(|room| room.token == "ben4chat").expect("ben");
        room.has_call = true;
        room.call_since = Some(at);
    }

    async fn call(chat: &Arc<ChatService>) -> ChatCall {
        match chat.handle(Request::ChatGetCall { conversation: ben() }, APP).await {
            Response::ChatCall { call } => call,
            other => panic!("a call: {other:?}"),
        }
    }

    #[tokio::test]
    async fn a_call_rings_once_until_it_is_old_answered_or_declined() {
        let (chat, server) = service();
        assert!(chat.look_for_rings(Utc::now()).await.is_empty(), "nobody calls");
        let quiet = call(&chat).await;
        assert!(!quiet.active && !quiet.ringing && quiet.people == 0);
        assert_eq!(quiet.link, format!("{}/call/ben4chat", super::super::fake::SERVER.trim_end_matches('/')));

        ben_calls(&server, 5);
        assert_eq!(chat.look_for_rings(Utc::now()).await, [ben()], "the window opens");
        assert!(chat.look_for_rings(Utc::now()).await.is_empty(), "once per call");
        let ringing = call(&chat).await;
        assert!(ringing.active && ringing.ringing && !ringing.joined, "{ringing:?}");
        assert_eq!(ringing.people, 1);
        let listed = chat
            .handle(Request::ChatListConversations { account: None, unread_only: false, query: None, limit: None }, APP)
            .await;
        let Response::ChatConversations { conversations } = listed else { panic!("listed") };
        assert!(conversations.iter().any(|c| c.id == ben() && c.ringing && c.has_call), "the list says it rings");

        // Too old: Talk gave up ringing after 45 s.
        assert!(chat.look_for_rings(Utc::now() + chrono::TimeDelta::seconds(60)).await.is_empty());
        assert!(!call(&chat).await.ringing);

        // Answered on another device: the call runs, but it does not ring.
        ben_calls(&server, 1);
        server.state().in_calls.insert("ben4chat".into(), vec!["ben".into(), super::super::fake::ME.into()]);
        assert!(chat.look_for_rings(Utc::now()).await.is_empty());
        let joined = call(&chat).await;
        assert!(joined.joined && !joined.ringing);
    }

    #[tokio::test]
    async fn declining_ends_the_ring_on_the_server_and_leaving_changes_nothing() {
        let (chat, server) = service();
        ben_calls(&server, 2);
        chat.look_for_rings(Utc::now()).await;
        assert_eq!(chat.handle(Request::ChatDeclineCall { conversation: ben() }, APP).await, Response::Ok);
        assert_eq!(server.count("end_ring 41"), 1);
        assert!(server.state().rings.is_empty() && !call(&chat).await.ringing);
        assert!(call(&chat).await.active, "the others go on calling");
        assert_eq!(chat.handle(Request::ChatDeclineCall { conversation: ben() }, APP).await, Response::Ok, "again");

        // A missed call stays in the person's notifications.
        ben_calls(&server, 120);
        assert_eq!(chat.handle(Request::ChatDeclineCall { conversation: ben() }, APP).await, Response::Ok);
        assert_eq!(server.state().rings.len(), 1, "a missed call is no ring");

        let calls = server.state().calls.len();
        assert_eq!(chat.handle(Request::ChatLeaveCall { conversation: ben() }, APP).await, Response::Ok);
        assert!(server.state().calls[calls..].iter().all(|call| !call.starts_with("end_ring")), "nothing left");
        let gone = conversation_id(ACCOUNT, "nobody4here");
        let refused = chat.handle(Request::ChatLeaveCall { conversation: gone }, APP).await;
        assert_eq!(refused, error_response(&ChatError::NotFound));
    }

    #[tokio::test]
    async fn switched_off_nothing_rings() {
        let (chat, server) = service();
        ben_calls(&server, 2);
        chat.look_for_rings(Utc::now()).await;
        chat.stop_rings();
        assert!(!call(&chat).await.ringing);
    }
}
