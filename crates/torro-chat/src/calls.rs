//! A call in a conversation as the windows see it (stage 6b of `docs/chat.md`, issue #643): whether
//! one runs, whether it rings for the person, and where to take part in the browser. The wire type
//! of `chat.get_call` (`torro_ipc::chat::ChatCall`) carries the same facts; the core's client turns
//! the one into the other.

use chrono::{DateTime, Utc};

/// The call of a conversation ([`crate::ChatSource::call`]). No names, no devices, no pictures.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Call {
    /// The conversation (`<account>/<token>`).
    pub conversation: String,
    /// A call runs.
    pub active: bool,
    /// Someone calls the person and it still rings for them: not declined, not answered on another
    /// device, at most 45 seconds.
    pub ringing: bool,
    /// When the call began, when the server says.
    pub since: Option<DateTime<Utc>>,
    /// How many people are in the call.
    pub people: u32,
    /// The person is in the call, on any of their devices.
    pub joined: bool,
    /// Where the person takes part in the browser (`<server>/call/<token>`): what "Accept" opens until
    /// the suite has its own call window.
    pub link: String,
}

impl Call {
    /// Whether the ringing window stays: it still rings, and the person has not joined elsewhere.
    pub fn rings(&self) -> bool {
        self.ringing && !self.joined
    }
}
