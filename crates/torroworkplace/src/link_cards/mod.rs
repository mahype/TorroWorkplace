//! Link cards for every window (issue #784, `docs/inhaltskarten.md`): what a web address in a chat message or an
//! answer of the assistant is ([`classify`]) and which addresses of a text get a card ([`links`]). The chat with
//! people (`crate::chat::attachments`) and the assistant (`crate::assistant::cards::link`) both go through here, so a
//! page looks the same in every window.
//!
//! - [`web`]: a page of any server, its card through the own Nextcloud's references and Settings › Privacy (strand
//!   2b of issue #784 moves `resolve_reference` and the privacy decision here from `chat/attachments.rs`),
//! - [`nextcloud`]: a file of the own Nextcloud server (strand 2c, issue #790),
//! - [`chat`]: a message of the own server's chat, quoted (strand 2c),
//! - [`github`]: an issue or pull request on GitHub, through the connected account (strand 2c).
//!
//! Every text a card shows is foreign: data, never instructions. Nothing here logs an address.

// The strands of issue #784 call these; the coordination removes this once they do.
#![allow(dead_code, reason = "scaffold of issue #784: the parsers and their callers come with strands 2b and 2c")]

pub(crate) mod chat;
pub(crate) mod github;
pub(crate) mod nextcloud;
pub(crate) mod web;

use torro_chat::LinkCardKind;

/// At most this many addresses of one text get a card (issue #785): the chat with people shows up to three cards per
/// message, the assistant up to three after an answer ([`torro_ipc::assistant::MAX_LINK_CARDS`]).
pub const MAX_LINKS: usize = torro_ipc::assistant::MAX_LINK_CARDS;

/// What a web address is (issue #784), by its form alone: nothing is fetched to tell it. Whether a card can be made
/// (an account, the rights, the server's answer) is decided when it is resolved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LinkKind {
    /// A page of any server, a foreign Nextcloud and GitHub without a connected account as well ([`web`]).
    Web,
    /// A file of the own Nextcloud server ([`nextcloud`]).
    NextcloudFile(FileLink),
    /// A message of the own server's chat: `/call/<token>#message_<id>` ([`chat`]).
    ChatMessage {
        /// The conversation's token.
        token: String,
        /// The message's number in it.
        message: u64,
    },
    /// An issue or pull request on GitHub: `github.com/<owner>/<repo>/(issues|pull)/<number>` ([`github`]).
    Github {
        /// `<owner>/<repo>`.
        repo: String,
        number: u64,
        /// `/pull/`, not `/issues/`.
        is_pull: bool,
    },
}

/// How a link names a file of the own Nextcloud ([`LinkKind::NextcloudFile`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FileLink {
    /// `/f/<id>`: the file's id.
    Id(u64),
    /// `/s/<token>`: a public share.
    Share(String),
    /// `/index.php/apps/files?dir=…` with the file it opens (`openfile`, `scrollto`), when it names one.
    Files { dir: String, file: Option<String> },
}

impl LinkKind {
    /// The kind of the card it becomes ([`torro_chat::LinkCard::kind`]).
    pub fn card_kind(&self) -> LinkCardKind {
        match self {
            LinkKind::Web => LinkCardKind::Web,
            LinkKind::NextcloudFile(_) => LinkCardKind::NextcloudFile,
            LinkKind::ChatMessage { .. } => LinkCardKind::ChatMessage,
            LinkKind::Github { .. } => LinkCardKind::Github,
        }
    }
}

/// What `link` is, `own` being the origin of the own Nextcloud server (`https://cloud.example.org`, `None` without
/// a Nextcloud account): a file or a chat message only on the own server ([`nextcloud::parse`], [`chat::parse`]), an
/// issue or pull request on `github.com` ([`github::parse`]), else – and for anything that is no `https` address – a
/// page ([`LinkKind::Web`]).
pub fn classify(link: &str, own: Option<&str>) -> LinkKind {
    let Ok(url) = url::Url::parse(link) else { return LinkKind::Web };
    if url.scheme() != "https" {
        return LinkKind::Web;
    }
    let on_own = own.filter(|own| torro_nextcloud::same_origin(link, own));
    on_own
        .and_then(|own| nextcloud::parse(&url, own).or_else(|| chat::parse(&url, own)))
        .or_else(|| github::parse(&url))
        .unwrap_or(LinkKind::Web)
}

/// The `https` addresses of `text` that may get a card (issue #785): at most [`MAX_LINKS`], in the order they stand,
/// each once, without the punctuation after them (`.`, `)`, `"` …). An address without a host is left out.
pub fn links(text: &str) -> Vec<String> {
    let mut found: Vec<String> = Vec::new();
    let mut rest = text;
    while found.len() < MAX_LINKS {
        let Some(start) = rest.find("https://") else { break };
        let tail = &rest[start..];
        let end = tail.find(|c: char| c.is_whitespace() || c.is_control()).unwrap_or(tail.len());
        let link = tail[..end].trim_end_matches(['.', ',', ';', ':', '!', '?', ')', ']', '}', '>', '"', '\'']);
        let fits = url::Url::parse(link).is_ok_and(|url| url.host_str().is_some());
        if fits && !found.iter().any(|seen| seen == link) {
            found.push(link.to_owned());
        }
        rest = &tail[end.max("https://".len())..];
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Issue #785: up to three addresses, in order, each once, without the punctuation after them.
    #[test]
    fn a_text_gives_up_to_three_links_in_order() {
        let text = "See https://a.example.org/x, (https://b.example.org) and https://a.example.org/x again; \
                    then https://c.example.org/y. And https://d.example.org/z";
        assert_eq!(links(text), ["https://a.example.org/x", "https://b.example.org", "https://c.example.org/y"]);
        assert_eq!(links("http://plain.example.org and https:// nothing"), Vec::<String>::new());
        assert_eq!(links("Only https://one.example.org/"), ["https://one.example.org/"]);
    }

    /// Without a parser that knows it, every address is a page; anything that is no `https` address too.
    #[test]
    fn an_address_nobody_knows_is_a_page() {
        let own = Some("https://cloud.example.org");
        assert_eq!(classify("https://example.org/blog", own), LinkKind::Web);
        assert_eq!(classify("http://cloud.example.org/f/12", own), LinkKind::Web, "not https");
        assert_eq!(classify("no address", None), LinkKind::Web);
        assert_eq!(LinkKind::Github { repo: "o/r".into(), number: 1, is_pull: true }.card_kind(), LinkCardKind::Github);
    }
}
