//! Files of the own Nextcloud server (issue #790, `docs/inhaltskarten.md` "Nextcloud-Datei"): their links
//! ([`parse`]), the own server a link lies on ([`own_server`]) and their cards ([`resolve`]: name, type, folder,
//! the server's preview), without Settings › Privacy, since no foreign provider is asked; only for a reader who
//! may read the chat of that Nextcloud account ([`reads_files`]).

use torro_chat::{ChatError, LinkCard, LinkCardKind, LinkFile};
use url::Url;

use super::{FileLink, LinkKind};
use crate::chat::ChatService;
use crate::chat::remote::FileError;
use crate::pim::Reach;
use torro_ipc::Request;

/// What `url` on the own server `own` names as a file ([`super::FileLink`]); `None` for any other address. `url` is
/// on `own` already ([`super::classify`] checked). Each form also below `/index.php`:
///
/// - `/f/<id>` and `/apps/files/files/<id>` (with or without `?dir=…&openfile=true`): [`FileLink::Id`];
/// - `/s/<token>` (and below it, `/s/<token>/download`): [`FileLink::Share`];
/// - `/apps/files?dir=<folder>` with the file it opens (`openfile=<name>` or `scrollto=<name>`), or with
///   `fileid=<id>` ([`FileLink::Id`] then): [`FileLink::Files`]. Without `dir` or a file id it is the app, no file.
pub fn parse(url: &Url, own: &str) -> Option<LinkKind> {
    let path = path_below(url, own)?;
    let segments: Vec<&str> = path.trim_matches('/').split('/').collect();
    let file = match segments.as_slice() {
        ["f", id] => FileLink::Id(id_of(id)?),
        ["s", token, ..] if is_share_token(token) => FileLink::Share((*token).to_owned()),
        ["apps", "files", "files", id] => FileLink::Id(id_of(id)?),
        ["apps", "files"] | ["apps", "files", "files"] => files_query(url)?,
        _ => return None,
    };
    Some(LinkKind::NextcloudFile(file))
}

/// The path of `url` below the own server `own` (`https://example.org/nextcloud` serves below `/nextcloud`), without
/// `/index.php` in front; `None` when `url` is not below it. Shared with [`super::chat::parse`].
pub(super) fn path_below(url: &Url, own: &str) -> Option<String> {
    let base = Url::parse(own).ok()?;
    let base = base.path().trim_end_matches('/');
    let rest = url.path().strip_prefix(base)?;
    if !rest.starts_with('/') {
        return None;
    }
    let rest = rest.strip_prefix("/index.php").map_or(rest, |below| below);
    Some(if rest.is_empty() { "/".to_owned() } else { rest.to_owned() })
}

/// `?dir=…` of a link to the files app: the folder with the file it opens, or the file id it names.
fn files_query(url: &Url) -> Option<FileLink> {
    let value = |key: &str| url.query_pairs().find(|(name, _)| name == key).map(|(_, value)| value.into_owned());
    if let Some(id) = value("fileid").as_deref().and_then(id_of) {
        return Some(FileLink::Id(id));
    }
    let dir = value("dir")?;
    // `openfile=true` (newer servers) only says to open the file the path names; a name is a file.
    let named = |value: String| (!value.is_empty() && value != "true" && value != "false").then_some(value);
    let file = value("openfile").and_then(named).or_else(|| value("scrollto").and_then(named));
    Some(FileLink::Files { dir, file })
}

/// A file id: digits only, more than zero.
fn id_of(text: &str) -> Option<u64> {
    let digits = !text.is_empty() && text.bytes().all(|byte| byte.is_ascii_digit());
    digits.then(|| text.parse().ok()).flatten().filter(|id| *id > 0)
}

/// A token of a public share as Nextcloud makes them (letters, digits, `-`, `_`), at most 64 characters.
fn is_share_token(token: &str) -> bool {
    (1..=64).contains(&token.len()) && token.bytes().all(|byte| byte.is_ascii_alphanumeric() || b"-_".contains(&byte))
}

/// A chat account whose server is the own Nextcloud a link lies on ([`own_server`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OwnServer {
    /// The account's id (`nextcloud:anna@cloud.example.org`), for [`resolve`] and [`super::chat::resolve`].
    pub account: String,
    /// Its server's origin, the `own` of [`super::classify`].
    pub server: String,
}

/// The first chat account of `chat` whose server `link` lies on, in the order of [`ChatService::accounts`]; `None`
/// when it lies on none (no Nextcloud account, or another server). Asks no server: the servers' addresses are known.
///
/// The assistant (strand 2b, `crate::assistant::cards::link`) calls it to classify an address
/// (`classify(url, own.map(|own| own.server))`) and to name the account for [`resolve`] and
/// [`super::chat::resolve`]; the chat with people knows the account of its conversation and needs it not.
pub async fn own_server(chat: &ChatService, link: &str) -> Option<OwnServer> {
    for account in chat.accounts().await {
        let Ok(remote) = chat.remote(&account).await else { continue };
        let server = remote.server();
        if torro_nextcloud::same_origin(link, &server) {
            return Some(OwnServer { account, server });
        }
    }
    None
}

/// Whether `reach` may see files of the own Nextcloud of the chat account `account`: the app key (the person)
/// always; a client when its rights let it read the chat of `account` – `chat.get_messages` allowed (read access to
/// `chat`, the command not switched off) and the account reached
/// ([`torro_ipc::api::Rights::reaches_chat_account`]), as [`super::chat::reads_conversation`] asks.
///
/// The catalogue has no area for the files of the own Nextcloud (the tool `files` is the computer's own folders).
/// The chat right stands in for it, decided in issue #784: the card is resolved through the same Nextcloud account
/// the client may read the chat of, and a link on that server reaches it in a chat message anyway.
pub fn reads_files(reach: Reach<'_>, account: &str) -> bool {
    let Reach::Client(rights) = reach else { return true };
    // Any conversation of the account: the right is the account's, not one conversation's.
    let read = Request::ChatGetMessages {
        conversation: torro_chat::conversation_id(account, "files"),
        before: None,
        after: None,
        around: None,
        limit: Some(1),
    };
    rights.allows_request(&read) && rights.reaches_chat_account(account)
}

/// The card of the file `file`, linked as `link` on the server of the chat account `account`, for `reach`.
///
/// - Checks [`reads_files`] first: without the right nothing is asked and the answer is `Ok(None)`.
/// - Then the own server resolves `link` (`references/resolve`; no Settings › Privacy, no foreign provider): its
///   name, its media type when the server names one, the folder of a `?dir=` link.
/// - `Ok(None)`: no card – no right, a link on another server than `account`'s, or a file the server does not show
///   the person (gone, not shared with them). The caller then shows **no card at all**, the address stays a link:
///   never a page card through [`super::web`], since the own server would name the file in it all the same.
/// - `Err`: the server cannot be asked now (offline, busy, sign-in refused); asked again with the next read.
///
/// The card's picture is the server's preview of the file (`Remote::preview` with the file id the link names,
/// [`FileLink::Id`]), fetched when a window asks for it, not here. Logs no address.
pub async fn resolve(
    chat: &ChatService,
    reach: Reach<'_>,
    account: &str,
    link: &str,
    file: &FileLink,
) -> Result<Option<LinkCard>, ChatError> {
    if !reads_files(reach, account) {
        return Ok(None);
    }
    let remote = match chat.remote(account).await {
        Ok(remote) => remote,
        Err(ChatError::NotFound) => return Ok(None),
        Err(err) => return Err(err),
    };
    if !torro_nextcloud::same_origin(link, &remote.server()) {
        return Ok(None);
    }
    let resolved = remote.resolve(link).await.map_err(|err| match err {
        FileError::Chat(err) => err,
        FileError::Exists | FileError::NoSpace | FileError::TooLarge => ChatError::NotFound,
    });
    match chat.asked(account, resolved).await {
        Ok(reference) => Ok(reference.map(|reference| card_of(link, file, reference))),
        Err(ChatError::NotFound | ChatError::Forbidden(_)) => Ok(None),
        Err(err) => Err(err),
    }
}

/// The card of `file` from what the own server resolved of `link`. For a file Nextcloud names it in the title and
/// its media type in the description (its file reference provider); any other description is no type.
fn card_of(link: &str, file: &FileLink, reference: torro_nextcloud::Reference) -> LinkCard {
    let content_type = reference.description.filter(|text| is_media_type(text)).unwrap_or_default();
    let folder = match file {
        FileLink::Files { dir, .. } => dir.trim_matches('/').to_owned(),
        FileLink::Id(_) | FileLink::Share(_) => String::new(),
    };
    let domain = Url::parse(link).ok().and_then(|url| url.host_str().map(str::to_owned)).unwrap_or_default();
    LinkCard {
        url: link.to_owned(),
        title: reference.title.clone(),
        description: None,
        domain,
        image: reference.image.is_some() || matches!(file, FileLink::Id(_)),
        kind: LinkCardKind::NextcloudFile,
        // The size is 0 ("not told"): the server's reference does not carry it to the core yet.
        file: Some(Box::new(LinkFile { name: reference.title, size: 0, content_type, folder })),
        ..LinkCard::default()
    }
}

/// `type/subtype` as media types are written (`application/pdf`, `httpd/unix-directory`), nothing else.
fn is_media_type(text: &str) -> bool {
    let part = |part: &str| !part.is_empty() && part.bytes().all(|b| b.is_ascii_alphanumeric() || b".+-_".contains(&b));
    text.split_once('/').is_some_and(|(kind, sub)| part(kind) && part(sub))
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use torro_ipc::api::RightsTemplate;

    use super::super::classify;
    use super::*;
    use crate::chat::fake::{SERVER, Server};

    const ACCOUNT: &str = "nextcloud:anna@cloud.example.org";

    fn kind(link: &str, own: &str) -> Option<LinkKind> {
        parse(&Url::parse(link).expect("an address"), own)
    }

    fn files(dir: &str, file: Option<&str>) -> Option<LinkKind> {
        Some(LinkKind::NextcloudFile(FileLink::Files { dir: dir.into(), file: file.map(str::to_owned) }))
    }

    /// Every form of a file link on the own server, below a sub-path too; the same links on a foreign server are
    /// pages.
    #[test]
    fn links_to_files_of_the_own_server_are_files_and_on_a_foreign_server_pages() {
        let own = "https://cloud.example.org";
        let id = |id| Some(LinkKind::NextcloudFile(FileLink::Id(id)));
        assert_eq!(kind("https://cloud.example.org/f/123", own), id(123));
        assert_eq!(kind("https://cloud.example.org/index.php/f/123", own), id(123));
        assert_eq!(kind("https://cloud.example.org/index.php/apps/files/files/45?dir=/A&openfile=true", own), id(45));
        assert_eq!(kind("https://cloud.example.org/index.php/apps/files/?dir=/A&fileid=77", own), id(77));
        assert_eq!(
            kind("https://cloud.example.org/s/AbC123xyz", own),
            Some(LinkKind::NextcloudFile(FileLink::Share("AbC123xyz".into())))
        );
        assert_eq!(
            kind("https://cloud.example.org/index.php/s/AbC123xyz/download", own),
            Some(LinkKind::NextcloudFile(FileLink::Share("AbC123xyz".into())))
        );
        assert_eq!(
            kind("https://cloud.example.org/index.php/apps/files?dir=/Projects/Customer&scrollto=Offer.pdf", own),
            files("/Projects/Customer", Some("Offer.pdf"))
        );
        assert_eq!(kind("https://cloud.example.org/apps/files/?dir=/Projects", own), files("/Projects", None));
        let sub = "https://example.org/nextcloud";
        assert_eq!(kind("https://example.org/nextcloud/index.php/f/9", sub), id(9));
        assert_eq!(kind("https://example.org/f/9", sub), None, "not below the server's path");

        for not_a_file in [
            "https://cloud.example.org/f/abc",
            "https://cloud.example.org/f/0",
            "https://cloud.example.org/index.php/apps/files",
            "https://cloud.example.org/index.php/apps/files/recent",
            "https://cloud.example.org/apps/calendar",
            "https://cloud.example.org/s/",
        ] {
            assert_eq!(kind(not_a_file, own), None, "{not_a_file}");
        }

        assert_eq!(
            classify("https://cloud.example.org/f/123", Some(own)),
            LinkKind::NextcloudFile(FileLink::Id(123)),
            "the own server"
        );
        assert_eq!(classify("https://cloud.other.org/f/123", Some(own)), LinkKind::Web, "a foreign Nextcloud");
        assert_eq!(classify("https://cloud.example.org/f/123", None), LinkKind::Web, "without an own server");
    }

    fn service() -> (ChatService, Arc<Server>) {
        let server = Arc::new(Server::default());
        let reference = torro_nextcloud::Reference {
            title: "Offer.pdf".into(),
            description: Some("application/pdf".into()),
            image: None,
        };
        server.state().references.insert(format!("{SERVER}/f/123"), reference);
        (ChatService::fixed(vec![(ACCOUNT.into(), Arc::clone(&server))]), server)
    }

    #[tokio::test]
    async fn the_person_gets_the_file_card_and_an_unknown_file_none() {
        let (chat, _server) = service();
        let link = format!("{SERVER}/f/123");
        let own = own_server(&chat, &link).await.expect("on the own server");
        assert_eq!(own, OwnServer { account: ACCOUNT.into(), server: SERVER.into() });
        assert_eq!(own_server(&chat, "https://cloud.other.org/f/123").await, None);

        let card = resolve(&chat, Reach::Suite, ACCOUNT, &link, &FileLink::Id(123)).await.expect("asked");
        let card = card.expect("a card");
        assert_eq!(
            (card.kind, card.title.as_str(), card.domain.as_str()),
            (LinkCardKind::NextcloudFile, "Offer.pdf", "cloud.example.org")
        );
        let file = card.file.expect("details");
        assert_eq!((file.name.as_str(), file.content_type.as_str()), ("Offer.pdf", "application/pdf"));

        let gone = format!("{SERVER}/f/999");
        assert_eq!(resolve(&chat, Reach::Suite, ACCOUNT, &gone, &FileLink::Id(999)).await, Ok(None));
    }

    #[tokio::test]
    async fn a_folder_link_names_its_folder_and_a_description_is_no_type() {
        let (chat, server) = service();
        let link = format!("{SERVER}/index.php/apps/files?dir=/Projects/Customer&scrollto=Notes.txt");
        let reference = torro_nextcloud::Reference {
            title: "Notes.txt".into(),
            description: Some("Our notes".into()),
            image: None,
        };
        server.state().references.insert(link.clone(), reference);
        let file = FileLink::Files { dir: "/Projects/Customer".into(), file: Some("Notes.txt".into()) };
        let card = resolve(&chat, Reach::Suite, ACCOUNT, &link, &file).await.expect("asked").expect("a card");
        let details = card.file.expect("details");
        assert_eq!((details.folder.as_str(), details.content_type.as_str()), ("Projects/Customer", ""));
        assert!(!card.image, "no picture known for it");
    }

    /// A client without read access to the chat of the account (issue #784: the chat right stands in for files)
    /// gets no card, and the server is not asked; with it, the card comes.
    #[tokio::test]
    async fn a_client_without_the_chat_right_gets_nothing_and_nothing_is_asked() {
        let (chat, server) = service();
        let link = format!("{SERVER}/f/123");
        server.state().references.insert(
            link.clone(),
            torro_nextcloud::Reference { title: "Plan.pdf".into(), description: None, image: None },
        );
        let mut rights = RightsTemplate::FullAccess.rights();
        rights.chat_accounts = torro_ipc::api::Selection::Only(Vec::new());
        let card = resolve(&chat, Reach::Client(&rights), ACCOUNT, &link, &FileLink::Id(123)).await;
        assert_eq!(card, Ok(None));
        assert_eq!(server.count("resolve"), 0, "nothing asked before the right");
        rights.chat_accounts = torro_ipc::api::Selection::Only(vec![ACCOUNT.into()]);
        let card = resolve(&chat, Reach::Client(&rights), ACCOUNT, &link, &FileLink::Id(123)).await;
        assert_eq!(card.expect("asked").map(|card| card.title), Some("Plan.pdf".into()));
    }

    #[tokio::test]
    async fn an_unreachable_server_is_asked_again_later() {
        let (chat, server) = service();
        server.state().offline = true;
        let link = format!("{SERVER}/f/123");
        assert_eq!(resolve(&chat, Reach::Suite, ACCOUNT, &link, &FileLink::Id(123)).await, Err(ChatError::Offline));
    }
}
