//! Live updates for the conversations an app is looking at (decision 9 in `docs/chat.md`).
//!
//! `chat.get_messages` with `after` from the app key notes a conversation for
//! [`super::cache::WATCHED_FOR`]. For each noted conversation the core holds one long-polling
//! request open, at most [`MAX_LIVE`] at once; what comes goes into the cache, and the app's next
//! look every two seconds is a look into memory. A conversation nobody asked about for 90 seconds
//! loses its request, and the next one waiting takes the place.
//!
//! Clients never note anything: they read the cache, so no agent can take the places of the
//! windows or keep requests open on the server.

use std::sync::Arc;
use std::time::{Duration, Instant};

use torro_chat::{ChatError, split_conversation};
use torro_nextcloud::RoomToken;

use super::ChatService;

/// The most long-polling requests open at once.
pub const MAX_LIVE: usize = 3;

/// How long one request waits on the server for something new.
const WAIT_SECONDS: u32 = 30;

/// How long to wait before asking again after the server could not be reached.
const RETRY: Duration = Duration::from_secs(5);

/// Starts a live request for each conversation that waits for one, while places are free.
pub fn start(service: &Arc<ChatService>) {
    let starting: Vec<String> = {
        let mut cache = service.cache();
        let free = MAX_LIVE.saturating_sub(cache.polling());
        let waiting: Vec<String> = cache.waiting(Instant::now()).into_iter().take(free).collect();
        for conversation in &waiting {
            if let Some(thread) = cache.peek_mut(conversation) {
                thread.polling = true;
            }
        }
        waiting
    };
    for conversation in starting {
        tokio::spawn(follow(Arc::clone(service), conversation));
    }
}

/// Keeps `conversation` current until nobody watches it any more, then gives its place away.
async fn follow(service: Arc<ChatService>, conversation: String) {
    while service.cache().peek(&conversation).is_some_and(|thread| thread.watched(Instant::now())) {
        match turn(&service, &conversation).await {
            Ok(()) => {}
            // No connection or a busy server: the conversation stays noted, try again shortly.
            Err(ChatError::Offline | ChatError::Busy) => tokio::time::sleep(RETRY).await,
            // Gone, signed out, no chat: nothing more will come.
            Err(_) => break,
        }
    }
    if let Some(thread) = service.cache().peek_mut(&conversation) {
        thread.polling = false;
        thread.watched_until = None;
    }
    start(&service);
}

/// One request: the newest messages when none are held yet, else a wait for what comes after the
/// newest one held.
async fn turn(service: &ChatService, conversation: &str) -> Result<(), ChatError> {
    let (account, token) = split_conversation(conversation).ok_or(ChatError::NotFound)?;
    let token = RoomToken::parse(token).ok_or(ChatError::NotFound)?;
    let (remote, caps, _) = service.ready(account).await?;
    let (fetched, newest) = {
        let cache = service.cache();
        let thread = cache.peek(conversation);
        (thread.is_some_and(|thread| thread.fetched.is_some()), thread.and_then(|thread| thread.newest()))
    };
    if !fetched {
        return service.fetch_newest(account, &token, &remote, &caps).await;
    }
    // An empty conversation waits from the start: number 0 is before every message.
    let after = newest.unwrap_or(0);
    let new = service.asked(account, remote.wait(token.as_str(), after, WAIT_SECONDS, &caps).await).await?;
    let mut cache = service.cache();
    if let Some(thread) = cache.peek_mut(conversation) {
        thread.merge(new);
        thread.fetched = Some(Instant::now());
    }
    Ok(())
}
