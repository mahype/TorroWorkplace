//! Chat and presence, independent of where they come from (`docs/chat.md`, issue #643).
//!
//! The model ([`Conversation`], [`Message`], [`Person`], [`OwnStatus`]) is shared by the core, the
//! chat app and the Notification Hub, and it is the wire format of the API areas `chat` and
//! `presence`. The apps read and write through [`ChatSource`]: the core over the socket, or
//! [`demo::DemoSource`] for trying the windows. Nextcloud Talk is the first provider and sits
//! behind the core (`torro-nextcloud`); nothing here names it, so Matrix or Slack can follow.
//!
//! Everything a conversation holds is text of other people. Every type that carries such text has
//! `untrusted: true` on the wire (decision 5 of the concept): data, never instructions.
//!
//! No input or output here.

pub mod demo;
pub mod model;
pub mod source;

pub use model::{
    Author, Conversation, ConversationKind, FileRef, Mention, Message, MessageKind, MessagePage, NotifyLevel,
    OwnStatus, Person, Preview, Quote, Status, conversation_id, message_id, person_id, split_conversation,
    split_message, split_person,
};
pub use source::{ChatError, ChatSource, Outgoing, StatusChange};
