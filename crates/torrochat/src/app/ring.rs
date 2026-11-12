//! The ringing window (`torrochat --ring=<conversation>`, class `torro-chat-ring`, stage 6b of
//! `docs/chat.md`, decision 24): someone calls, the window in the middle says who and offers
//! "Decline", "Audio only" and "Accept".
//!
//! - **Accept** and **Audio only** open the call in the browser (`link` of `chat.get_call`, decision
//!   5) – the suite has no call window of its own yet – and close the window.
//! - **Decline** sends `chat.decline_call` and closes the window; what the core refuses stays as a
//!   calm line, and `Esc` closes without declining.
//! - **Every [`RING_POLL`]** the window asks `chat.get_call` and closes when it no longer rings or
//!   the person joined on another device; after [`RING_FOR`] it closes by itself.

use std::time::{Duration, Instant};

use iced::{Subscription, Task};
use torro_chat::{Call, ChatError};
use torro_core::i18n::tr;

use super::{App, Message, chat_failed};
use crate::system;

/// How often the window asks whether it still rings.
pub const RING_POLL: Duration = Duration::from_millis(2500);

/// How long a call rings at most (Talk: 45 s): then the window goes by itself.
pub const RING_FOR: Duration = Duration::from_secs(45);

/// What the window is doing after a click.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RingBusy {
    Declining,
}

/// The state of the ringing window.
#[derive(Debug)]
pub struct RingState {
    /// The conversation that rings (`<account>/<token>`).
    pub conversation: String,
    /// What the core said last; `None` before its first answer.
    pub call: Option<Call>,
    /// When the window opened: [`RING_FOR`] counts from here.
    pub opened: Instant,
    /// An ask for the call is out.
    pub asking: bool,
    pub busy: Option<RingBusy>,
    /// Why a click did not go through, in words for people.
    pub error: Option<String>,
}

/// What happens in the ringing window.
#[derive(Debug, Clone)]
pub enum RingMsg {
    Tick,
    Called(Result<Call, ChatError>),
    /// "Accept" or "Audio only": both open the call in the browser, which asks for the camera itself.
    Accept,
    Decline,
    Declined(Result<(), ChatError>),
}

impl RingState {
    pub fn new(conversation: String) -> RingState {
        RingState { conversation, call: None, opened: Instant::now(), asking: false, busy: None, error: None }
    }

    /// The call's link once the core named it.
    pub fn link(&self) -> Option<&str> {
        self.call.as_ref().map(|call| call.link.as_str()).filter(|link| !link.is_empty())
    }

    pub fn subscription(&self) -> Subscription<RingMsg> {
        iced::time::every(RING_POLL).map(|_| RingMsg::Tick)
    }
}

/// What a fresh answer of the core means for the window: whether it stays.
pub fn stays(result: &Result<Call, ChatError>) -> bool {
    match result {
        Ok(call) => call.rings(),
        Err(ChatError::NotFound) => false,
        // Offline or busy: ask again at the next tick.
        Err(_) => true,
    }
}

/// Closes the window, and with it this process.
fn close() -> Task<Message> {
    iced::window::latest().and_then(iced::window::close)
}

impl App {
    /// The first ask of the ringing window.
    pub(crate) fn ring_start(&mut self) -> Task<Message> {
        if self.ring.is_none() {
            return Task::none();
        }
        self.ring_update(RingMsg::Tick)
    }

    /// What happens in the ringing window.
    pub(crate) fn ring_update(&mut self, msg: RingMsg) -> Task<Message> {
        let backend = self.backend.clone();
        let demo = backend.is_demo();
        let Some(ring) = self.ring.as_mut() else { return Task::none() };
        match msg {
            RingMsg::Tick => {
                if ring.opened.elapsed() >= RING_FOR {
                    return close();
                }
                if ring.asking {
                    return Task::none();
                }
                ring.asking = true;
                let conversation = ring.conversation.clone();
                return Task::perform(backend.call(conversation), |result| Message::Ring(RingMsg::Called(result)));
            }
            RingMsg::Called(result) => {
                ring.asking = false;
                if !stays(&result) {
                    return close();
                }
                if let Ok(call) = result {
                    ring.call = Some(call);
                }
            }
            RingMsg::Accept => {
                let Some(link) = ring.link().map(str::to_owned) else { return Task::none() };
                // The demo's server does not exist: nothing to open.
                if demo {
                    return close();
                }
                return match system::open_link(&link) {
                    Ok(()) => close(),
                    Err(why) => {
                        ring.error = Some(why);
                        Task::none()
                    }
                };
            }
            RingMsg::Decline => {
                if ring.busy.is_some() {
                    return Task::none();
                }
                ring.busy = Some(RingBusy::Declining);
                ring.error = None;
                let conversation = ring.conversation.clone();
                return Task::perform(backend.decline_call(conversation), |result| {
                    Message::Ring(RingMsg::Declined(result))
                });
            }
            RingMsg::Declined(result) => {
                ring.busy = None;
                match result {
                    Ok(()) | Err(ChatError::NotFound) => return close(),
                    Err(err) => ring.error = Some(chat_failed(&err)),
                }
            }
        }
        Task::none()
    }

    /// The name of the one who calls: the other person of a direct chat, else the conversation.
    pub(crate) fn caller(&self, ring: &RingState) -> String {
        match self.conversations.iter().find(|conversation| conversation.id == ring.conversation) {
            Some(conversation) => conversation.name.clone(),
            None => tr("Incoming call").to_owned(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_window_stays_only_while_it_rings() {
        let ringing = Call { ringing: true, ..Call::default() };
        assert!(stays(&Ok(ringing.clone())));
        assert!(!stays(&Ok(Call { joined: true, ..ringing })), "answered on another device");
        assert!(!stays(&Ok(Call::default())), "no longer ringing");
        assert!(!stays(&Err(ChatError::NotFound)));
        assert!(stays(&Err(ChatError::Offline)), "asked again at the next tick");
        let state = RingState::new("a/b".into());
        assert_eq!(state.link(), None);
    }
}
