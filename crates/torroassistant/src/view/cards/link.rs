//! The link card in the assistant's chat (issue #785, #790): a page, a video, a file of the own Nextcloud, a quoted
//! chat message or a GitHub issue, drawn with `content_card::link_card`; or, while the person was not asked yet, the
//! question "Show once", "Always allow …", "Never for …" (`content_card::ask_card`).
//!
//! The buttons: the card opens the address ([`Message::OpenLink`]; a video opens there too, the assistant has no
//! player), "Show once" is `assistant.show_card` with the entry's number, "Always allow …" and "Never for …" are
//! `settings.set_setting privacy_provider`, each a [`Message::CardRequest`]. The picture comes through
//! `assistant.get_card_picture` once per card ([`App::fetch_card_picture`]); until then the kind's icon stands in its
//! place.

use iced::Element;
use iced::widget::image;
use torro_core::i18n::tr;
use torro_gui::content_card::{self, Action, AskPress, ContentCard, Head, LinkPress};
use torro_gui::icon::Icon;
use torro_ipc::assistant::{ForeignProvider, LinkCard};
use torro_ipc::{Request, SettingKey};

use super::card_request;
use crate::app::{App, Message};

/// "Always allow …" (`allowed`) or "Never for …" (`blocked`) for the provider `key`, as Settings › Privacy keeps it.
fn decide(key: &str, choice: &str) -> Message {
    let value = serde_json::json!({"provider": key, "choice": choice});
    let request = Request::SetSetting { key: SettingKey::PrivacyProvider, value };
    card_request(request, tr("Couldn't change Settings › Privacy."))
}

/// The host of `url` without `www.`, for the head of a link card the core has not resolved: "example.org"; the
/// address itself when it has no host.
pub(super) fn domain(url: &str) -> String {
    let rest = url.split_once("://").map_or(url, |(_, rest)| rest);
    let host = rest.split(['/', '?', '#']).next().unwrap_or(rest);
    let host = host.rsplit_once('@').map_or(host, |(_, host)| host);
    match host.strip_prefix("www.").unwrap_or(host) {
        "" => url.to_owned(),
        host => host.to_owned(),
    }
}

/// The icon of a link card's kind, for its line, told by the details it carries (each kind has its own).
fn kind_icon(card: &LinkCard) -> Icon {
    if card.github.is_some() {
        Icon::Github
    } else if card.file.is_some() {
        Icon::Cloud
    } else if card.quote.is_some() {
        Icon::MessageCircle
    } else if card.video.is_some() {
        Icon::Video
    } else {
        Icon::Globe
    }
}

impl App {
    /// What the three answers of the question about `provider` send for the entry `seq`: "Show once"
    /// (`assistant.show_card` of the open conversation), "Always allow …" and "Never for …"; `None` without an open
    /// conversation.
    fn ask_press(&self, seq: u64, provider: &ForeignProvider) -> Option<AskPress<Message>> {
        let conversation = self.current.clone()?;
        let once = Request::ShowAssistantCard { conversation, seq };
        Some(AskPress {
            once: card_request(once, tr("Couldn't load the preview.")),
            always: decide(&provider.key, "allowed"),
            never: decide(&provider.key, "blocked"),
        })
    }

    /// The buttons of the link card of the entry `seq`, the main one first: "Open" with a card, the three answers
    /// with the question, nothing with neither: what [`App::link_card`] draws, for the tests of the buttons.
    #[cfg(test)]
    pub(super) fn link_actions(
        &self,
        seq: u64,
        url: &str,
        card: Option<&LinkCard>,
        ask: Option<&ForeignProvider>,
    ) -> Vec<Action<Message>> {
        if card.is_some() {
            return vec![Action::new(Icon::ArrowUpRight, tr("Open"), Message::OpenLink(url.to_owned()))];
        }
        let Some((provider, press)) = ask.and_then(|provider| Some((provider, self.ask_press(seq, provider)?))) else {
            return Vec::new();
        };
        use torro_core::i18n::trf;
        let name = provider.name.as_str();
        vec![
            Action::new(Icon::Eye, tr("Show once"), press.once),
            Action::new(Icon::Check, trf("Always allow {provider}", &[("provider", &name)]), press.always),
            Action::new(Icon::X, trf("Never for {provider}", &[("provider", &name)]), press.never),
        ]
    }

    /// The link card of the entry `seq` for `url`, at most `room` wide: the card `card` with its picture `picture`
    /// (the window holds it, by conversation and `seq`), or the question about `ask`; `None` with neither – the
    /// address stays a link in the answer.
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
        Some(content_card::ask_card_in(provider, self.ask_press(seq, provider)?, room))
    }

    /// The link card of `url` in one line: its kind's icon, its title or domain, and "Open"; `None` as
    /// [`App::link_card`] without a card.
    pub(super) fn link_line<'a>(&'a self, url: &'a str, card: Option<&'a LinkCard>) -> Option<Element<'a, Message>> {
        let card = card?;
        let title = if card.title.trim().is_empty() { card.domain.clone() } else { card.title.clone() };
        let open = Action::new(Icon::ArrowUpRight, tr("Open"), Message::OpenLink(url.to_owned()));
        Some(ContentCard::new(Head::new(kind_icon(card), card.domain.clone()), title).main(open).short())
    }
}
