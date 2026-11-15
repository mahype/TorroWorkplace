//! Pages of any server (issue #672, #785): who provides a page for Settings › Privacy ([`provider`]), whether its
//! card loads by itself ([`decide`], [`turned_off`]) and its card through the own Nextcloud's references
//! ([`resolve`]), for the chat with people (`crate::chat::attachments`) and the assistant
//! (`crate::assistant::cards::link`) alike.
//!
//! The own server fetches the address, so the page's owner might learn when it was read: that is why a page of
//! another server follows Settings › Privacy, and why nothing here fetches a page itself or logs an address.

use torro_chat::LinkCard;
use torro_core::privacy::{Decision, Privacy};

use crate::chat::remote::{FileError, Remote};

/// The provider of `link` as Settings › Privacy names it: its host as the suite compares hosts
/// ([`torro_core::privacy::normalize_host`]); empty for an address without one.
pub fn provider(link: &str) -> String {
    let host = url::Url::parse(link).ok().and_then(|url| url.host_str().map(str::to_owned)).unwrap_or_default();
    torro_core::privacy::normalize_host(&host)
}

/// Whether the card of a link may load (issue #672).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Decided {
    /// To the own server: its card loads for the person by itself, without Settings › Privacy.
    Own,
    /// To another server: what Settings › Privacy decides for its provider.
    Other(Decision),
}

impl Decided {
    /// Whether its card loads by itself.
    pub fn loads(&self) -> bool {
        matches!(self, Decided::Own | Decided::Other(Decision::Load))
    }
}

/// What `link` is for its card, `server` being the own Nextcloud's address (`None` signed out, when every link
/// counts as another server's): [`Decided::Own`] on that server, else Settings › Privacy's decision for its
/// [`provider`].
pub fn decide(privacy: &Privacy, link: &str, server: Option<&str>) -> Decided {
    match server {
        Some(server) if torro_nextcloud::same_origin(link, server) => Decided::Own,
        _ => Decided::Other(privacy.decide(&provider(link))),
    }
}

/// Whether Settings › Privacy turned the card of `link` off (issue #672): a link to another server than `server`
/// whose provider is blocked, or any such link at "Never show". An empty link (a card without one) counts as
/// another server's. A card held from before is left out then too.
pub fn turned_off(privacy: &Privacy, link: &str, server: &str) -> bool {
    decide(privacy, link, Some(server)) == Decided::Other(Decision::Skip)
}

/// The card of a link as the own server resolved it, and the id of its picture there (for `chat.get_preview` with
/// `card`, `assistant.get_card_picture`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Resolved {
    /// `None`: asked, the server knows none (so it is not asked again).
    pub card: Option<LinkCard>,
    /// The picture's id on the own server.
    pub image: Option<String>,
}

/// The card of `link` from what the server answered (`None`: it knows none). A YouTube video carries its id
/// (issue #673), also without a card of the server, so the video still plays; any other link without one has no
/// card.
pub fn resolved(link: &str, reference: Option<torro_nextcloud::Reference>) -> Resolved {
    let Some(reference) = reference else {
        let card = torro_chat::video_of(link).map(|video| LinkCard {
            url: link.to_string(),
            domain: domain_of(link),
            video: Some(video),
            ..LinkCard::default()
        });
        return Resolved { card, image: None };
    };
    Resolved {
        card: Some(LinkCard {
            url: link.to_string(),
            title: reference.title,
            description: reference.description,
            domain: domain_of(link),
            image: reference.image.is_some(),
            video: torro_chat::video_of(link),
            ..LinkCard::default()
        }),
        image: reference.image,
    }
}

/// The card of `link` as the own server `remote` resolves it (the server fetches the address): whether it may is
/// the caller's to decide first ([`decide`]). An error when the server cannot be reached now.
pub async fn resolve(remote: &Remote, link: &str) -> Result<Resolved, FileError> {
    Ok(resolved(link, remote.resolve(link).await?))
}

/// The host of `link` as it stands there (`youtu.be`); empty when it has none.
fn domain_of(link: &str) -> String {
    url::Url::parse(link).ok().and_then(|url| url.host_str().map(str::to_string)).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use torro_core::privacy::{ForeignContent, ProviderChoice};

    use super::*;

    const SERVER: &str = "https://cloud.example.org";

    #[test]
    fn the_provider_is_the_host() {
        assert_eq!(provider("https://example.org/a?b"), torro_core::privacy::normalize_host("example.org"));
        assert_eq!(provider("not an address"), "");
    }

    /// Issue #672: the own server is no provider; another follows Settings › Privacy, and only blocked or "Never
    /// show" turn a card off.
    #[test]
    fn a_page_of_another_server_follows_settings_privacy() {
        let mut privacy = Privacy::default();
        assert_eq!(decide(&privacy, "https://cloud.example.org/f/1", Some(SERVER)), Decided::Own);
        assert!(matches!(decide(&privacy, "https://example.org/a", Some(SERVER)), Decided::Other(Decision::Ask(_))));
        assert!(matches!(decide(&privacy, "https://cloud.example.org/f/1", None), Decided::Other(_)), "signed out");
        privacy.providers.insert("example.org".into(), ProviderChoice::Blocked);
        assert!(turned_off(&privacy, "https://example.org/a", SERVER));
        assert!(!turned_off(&privacy, "https://cloud.example.org/f/1", SERVER));
        privacy.content = ForeignContent::Always;
        assert!(decide(&privacy, "https://example.org/a", Some(SERVER)).loads());
        privacy.content = ForeignContent::Never;
        assert!(turned_off(&privacy, "https://other.example.net/", SERVER));
    }

    #[test]
    fn a_link_the_server_knows_no_card_for_has_none_unless_it_is_a_video() {
        assert_eq!(resolved("https://example.org/a", None), Resolved { card: None, image: None });
        let video = resolved("https://www.youtube.com/watch?v=dQw4w9WgXcQ", None).card.expect("a video card");
        assert!(video.video.is_some() && video.title.is_empty());
        let reference = torro_nextcloud::Reference { title: "A".into(), description: None, image: Some("p1".into()) };
        let page = resolved("https://example.org/a", Some(reference));
        assert_eq!(page.image.as_deref(), Some("p1"));
        assert!(page.card.is_some_and(|card| card.image && card.domain == "example.org"));
    }
}
