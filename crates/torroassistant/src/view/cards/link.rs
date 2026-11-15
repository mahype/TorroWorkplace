//! The link card in the assistant's chat (issue #785, #790): a page, a video, a file of the own Nextcloud, a quoted
//! chat message or a GitHub issue, drawn with `content_card::link_card`; or, while the person was not asked yet, the
//! question "Show once", "Always allow …", "Never for …" (`content_card::ask_card`).
//!
//! The buttons: the card opens the address ([`Message::OpenLink`]; a video plays there too until the assistant has a
//! player), "Show once" is `assistant.show_card` with the entry's number, "Always allow …" and "Never for …" are
//! `settings.set_setting privacy_provider`, each a [`Message::CardRequest`]. Strand 2g of issue #784 gives the card
//! its picture and final look.

use iced::Element;
use iced::widget::image;
use torro_core::i18n::tr;
use torro_gui::content_card::{self, AskPress, LinkPress};
use torro_ipc::assistant::{ForeignProvider, LinkCard};
use torro_ipc::{Request, SettingKey};

use crate::app::{App, Message};

/// "Always allow …" (`allowed`) or "Never for …" (`blocked`) for the provider `key`, as Settings › Privacy keeps it.
fn decide(key: &str, choice: &str) -> Message {
    let value = serde_json::json!({"provider": key, "choice": choice});
    let request = Request::SetSetting { key: SettingKey::PrivacyProvider, value };
    Message::CardRequest(Box::new(request), tr("Couldn't change Settings › Privacy."))
}

impl App {
    /// The link card of the entry `seq` for `url`, at most `room` wide: the card `card` with its picture `picture`
    /// (the window holds it, by `seq`), or the question about `ask`; `None` with neither – the address stays a link
    /// in the answer.
    pub(super) fn link_card<'a>(
        &'a self,
        seq: u64,
        url: &'a str,
        card: Option<&'a LinkCard>,
        ask: Option<&'a ForeignProvider>,
        picture: Option<&image::Handle>,
        room: f32,
    ) -> Option<Element<'a, Message>> {
        if let Some(card) = card {
            let press = LinkPress { open: Message::OpenLink(url.to_owned()), play: None };
            return Some(content_card::link_card(card, picture.cloned(), press, room));
        }
        let provider = ask?;
        let conversation = self.current.clone()?;
        let once = Request::ShowAssistantCard { conversation, seq };
        let press = AskPress {
            once: Message::CardRequest(Box::new(once), tr("Couldn't load the preview.")),
            always: decide(&provider.key, "allowed"),
            never: decide(&provider.key, "blocked"),
        };
        Some(content_card::ask_card(provider, press))
    }

    /// The link card of the entry `seq` in one line: its title or domain; `None` as [`App::link_card`].
    pub(super) fn link_line<'a>(&'a self, card: Option<&'a LinkCard>) -> Option<Element<'a, Message>> {
        let card = card?;
        let title = if card.title.trim().is_empty() { card.domain.clone() } else { card.title.clone() };
        let head = content_card::Head::new(torro_gui::icon::Icon::Globe, card.domain.clone());
        Some(content_card::ContentCard::<Message>::new(head, title).short())
    }
}
