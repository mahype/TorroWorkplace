//! Folders and their roles (task M2.3): which folder is the inbox, drafts, sent, archive, junk
//! and trash of an account, found anew on every sync ([`discover`]).
//!
//! Order, after TorroMail and `docs/mail.md` ("Ordner: Entscheidungen statt Optionen"):
//!
//! 1. a folder the person picked earlier for a role (`MailSettings::folder_roles`),
//! 2. `INBOX` for the inbox,
//! 3. the server's attributes: `LIST … RETURN (SPECIAL-USE)` or `XLIST` (`\Sent`, `\Spam`, …),
//! 4. known and translated names ([`known_name`]), compared with the last part of the path,
//!    modified UTF-7 decoded (`INBOX.Entw&APw-rfe` is "Entwürfe"),
//! 5. for the archive, Gmail's `\All` (`\AllMail` in XLIST; "All Mail": moving there takes mail
//!    out of the inbox).
//!
//! Two name candidates with content leave the role open ([`OpenRole`]); the account page asks
//! once (task M5.2) and the answer comes back as step 1. A missing drafts or sent folder is
//! created under the `NAMESPACE` prefix and subscribed, so saving a draft or the copy of a sent
//! mail never fails for want of a folder. Paths always stay as the server sends them; decoding
//! is for showing only ([`display_name`]).

use std::collections::BTreeMap;

use torro_mail::Role;

use crate::imap::{ImapError, ListedFolder, Session};

/// The roles in the order they are given out; a folder gets at most one, the earlier wins.
const ROLE_ORDER: [Role; 6] = [Role::Inbox, Role::Drafts, Role::Sent, Role::Trash, Role::Junk, Role::Archive];

/// Known names per role, most likely first, compared without regard to case with the decoded
/// last part of a path. From TorroMail (`common_names`), with the names of German, French,
/// Spanish, Italian and Dutch servers and of Outlook and Gmail added.
const KNOWN_NAMES: [(Role, &[&str]); 5] = [
    (Role::Drafts, &["Drafts", "Draft", "Entwürfe", "Brouillons", "Borradores", "Bozze", "Concepten"]),
    (
        Role::Sent,
        &[
            "Sent",
            "Sent Mail",
            "Sent Messages",
            "Sent Items",
            "Gesendet",
            "Gesendete Elemente",
            "Gesendete Objekte",
            "Envoyés",
            "Éléments envoyés",
            "Enviados",
            "Posta inviata",
            "Inviati",
            "Verzonden",
        ],
    ),
    (Role::Archive, &["Archive", "Archives", "Archiv", "Archiviert", "Archivio", "Archief"]),
    (
        Role::Junk,
        &[
            "Junk",
            "Spam",
            "Junk E-mail",
            "Junk Email",
            "Bulk Mail",
            "Unerwünscht",
            "Courrier indésirable",
            "Indesiderata",
        ],
    ),
    (
        Role::Trash,
        &[
            "Trash",
            "Deleted Items",
            "Deleted Messages",
            "Bin",
            "Papierkorb",
            "Gelöschte Elemente",
            "Gelöschte Objekte",
            "Corbeille",
            "Papelera",
            "Cestino",
            "Prullenbak",
        ],
    ),
];

/// One folder of an account with its role, as [`discover`] found it. The core gives it an id
/// and counts and turns it into a `torro_mail::Folder`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteFolder {
    /// The path exactly as the server names it; commands and the cache use this.
    pub path: String,
    /// The name to show ([`display_name`]): `Entwürfe` for `INBOX.Entw&APw-rfe`.
    pub name: String,
    /// The hierarchy delimiter; `None` for a flat server.
    pub delimiter: Option<char>,
    /// What the folder is for, if anything.
    pub role: Option<Role>,
    /// It holds messages (not only other folders); only these are synced.
    pub selectable: bool,
}

/// A role with more than one folder that could fill it, all with messages in them: the person
/// picks one (task M5.2, stored in `MailSettings::folder_roles`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenRole {
    pub role: Role,
    /// The paths of the candidates, in the order of [`discover`]'s list.
    pub candidates: Vec<String>,
}

/// What [`discover`] found.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct FolderList {
    /// Every folder, in the server's order, folders created by `discover` last.
    pub folders: Vec<RemoteFolder>,
    /// Roles left open for the person to pick.
    pub open: Vec<OpenRole>,
    /// Paths of the folders `discover` created (drafts, sent), for the core to report.
    pub created: Vec<String>,
}

impl FolderList {
    /// The path of the folder with `role`, if one has it.
    pub fn path_of(&self, role: Role) -> Option<&str> {
        self.folders.iter().find(|folder| folder.role == Some(role)).map(|folder| folder.path.as_str())
    }
}

/// Lists the folders of the signed-in `session` and gives them their roles (see the module
/// documentation for the order). `chosen` are the person's picks for roles that were open
/// (`MailSettings::folder_roles`); a pick whose folder is gone is ignored.
///
/// Sends `LIST`/`XLIST`, `STATUS` for name candidates when there is more than one, and, when no
/// drafts or sent folder exists and the role is not open, `NAMESPACE`, `CREATE` (with `USE`
/// where the server can) and `SUBSCRIBE`. A refused subscription is ignored; a refused creation
/// is an error, like any other the session reports.
pub async fn discover(session: &mut Session, chosen: &BTreeMap<Role, String>) -> Result<FolderList, ImapError> {
    let listed = session.list().await?;
    let mut roles: BTreeMap<Role, String> = BTreeMap::new();
    let mut open = Vec::new();

    for (&role, path) in chosen {
        if listed.iter().any(|folder| folder.selectable() && &folder.path == path) && !is_taken(&roles, path) {
            roles.insert(role, path.clone());
        }
    }
    if !roles.contains_key(&Role::Inbox)
        && let Some(inbox) = listed
            .iter()
            .find(|f| f.selectable() && f.path.eq_ignore_ascii_case("INBOX"))
            .or_else(|| listed.iter().find(|f| f.selectable() && f.has_attribute("\\Inbox")))
    {
        roles.insert(Role::Inbox, inbox.path.clone());
    }
    for role in ROLE_ORDER {
        if roles.contains_key(&role) {
            continue;
        }
        let by_attribute = listed
            .iter()
            .find(|f| f.selectable() && role_of_attributes(&f.attributes) == Some(role) && !is_taken(&roles, &f.path));
        if let Some(folder) = by_attribute {
            roles.insert(role, folder.path.clone());
        }
    }
    for role in ROLE_ORDER {
        if roles.contains_key(&role) {
            continue;
        }
        let mut candidates: Vec<(usize, &ListedFolder)> = listed
            .iter()
            .filter(|f| f.selectable() && !is_taken(&roles, &f.path))
            .filter_map(|f| match known_name(&f.path, f.delimiter) {
                Some((known, rank)) if known == role => Some((rank, f)),
                _ => None,
            })
            .collect();
        candidates.sort_by_key(|&(rank, _)| rank);
        match candidates.as_slice() {
            [] => {}
            [(_, only)] => {
                roles.insert(role, only.path.clone());
            }
            several => {
                let mut with_content = Vec::new();
                for (_, folder) in several {
                    if has_content(session, &folder.path).await? {
                        with_content.push(folder.path.clone());
                    }
                }
                match with_content.len() {
                    0 => {
                        roles.insert(role, several[0].1.path.clone());
                    }
                    1 => {
                        roles.insert(role, with_content.remove(0));
                    }
                    _ => open.push(OpenRole { role, candidates: with_content }),
                }
            }
        }
    }
    if !roles.contains_key(&Role::Archive)
        && !open.iter().any(|o| o.role == Role::Archive)
        && let Some(all) = listed.iter().find(|f| {
            f.selectable() && (f.has_attribute("\\All") || f.has_attribute("\\AllMail")) && !is_taken(&roles, &f.path)
        })
    {
        roles.insert(Role::Archive, all.path.clone());
    }

    let mut folders: Vec<RemoteFolder> = listed
        .iter()
        .map(|f| RemoteFolder {
            path: f.path.clone(),
            name: display_name(&f.path, f.delimiter),
            delimiter: f.delimiter,
            role: roles.iter().find(|(_, path)| **path == f.path).map(|(&role, _)| role),
            selectable: f.selectable(),
        })
        .collect();
    let mut created = Vec::new();
    let mut namespace = None;
    for role in [Role::Drafts, Role::Sent] {
        if roles.contains_key(&role) || open.iter().any(|o| o.role == role) {
            continue;
        }
        let namespace = match &namespace {
            Some(known) => known,
            None => namespace.insert(session.namespace().await?),
        };
        let path = format!("{}{}", namespace.prefix, if role == Role::Drafts { "Drafts" } else { "Sent" });
        // A folder of that name that cannot hold mail (`\Noselect`) would make CREATE fail:
        // better no role than an error on every sync.
        if listed.iter().any(|f| f.path == path) {
            continue;
        }
        session.create_folder(&path, Some(role)).await?;
        match session.subscribe(&path).await {
            Ok(()) | Err(ImapError::Protocol(_) | ImapError::NotFound(_)) => {}
            Err(error) => return Err(error),
        }
        folders.push(RemoteFolder {
            name: display_name(&path, namespace.delimiter),
            path: path.clone(),
            delimiter: namespace.delimiter,
            role: Some(role),
            selectable: true,
        });
        created.push(path);
    }
    Ok(FolderList { folders, open, created })
}

fn is_taken(roles: &BTreeMap<Role, String>, path: &str) -> bool {
    roles.values().any(|taken| taken == path)
}

/// The folder holds at least one message. A folder that vanished in between counts as empty.
async fn has_content(session: &mut Session, path: &str) -> Result<bool, ImapError> {
    match session.status(path).await {
        Ok(status) => Ok(status.total > 0),
        Err(ImapError::NotFound(_)) => Ok(false),
        Err(error) => Err(error),
    }
}

/// The role the server's attributes give a folder: RFC 6154 (`\Drafts`, `\Sent`, `\Archive`,
/// `\Junk`, `\Trash`) and the older XLIST (`\Inbox`, `\Spam`). `\All` is not a role here; see
/// [`discover`].
pub fn role_of_attributes(attributes: &[String]) -> Option<Role> {
    attributes.iter().find_map(|attribute| match attribute.to_ascii_lowercase().as_str() {
        "\\inbox" => Some(Role::Inbox),
        "\\drafts" => Some(Role::Drafts),
        "\\sent" => Some(Role::Sent),
        "\\archive" => Some(Role::Archive),
        "\\junk" | "\\spam" => Some(Role::Junk),
        "\\trash" => Some(Role::Trash),
        _ => None,
    })
}

/// The role a folder's name suggests, and how likely (0 = the most usual name): the decoded last
/// part of `path` compared without regard to case with the known names, so `INBOX.Sent`,
/// `[Gmail]/Sent Mail` and `Entw&APw-rfe` are found.
pub fn known_name(path: &str, delimiter: Option<char>) -> Option<(Role, usize)> {
    let name = display_name(path, delimiter).to_lowercase();
    KNOWN_NAMES
        .iter()
        .find_map(|(role, names)| names.iter().position(|known| known.to_lowercase() == name).map(|rank| (*role, rank)))
}

/// Providers that put a copy of every mail sent through their SMTP server into the sent folder
/// themselves (Gmail, Outlook.com and Microsoft 365): the core must not append a second one
/// (task M2.4). Decided by the IMAP host.
pub fn keeps_sent_copies(imap_host: &str) -> bool {
    let host = imap_host.trim().trim_end_matches('.').to_ascii_lowercase();
    ["gmail.com", "googlemail.com", "outlook.com", "office365.com"]
        .iter()
        .any(|domain| host == *domain || host.ends_with(&format!(".{domain}")))
}

/// The name to show for a folder: the last part of `path` after `delimiter`, modified UTF-7
/// decoded ([`decode_utf7`]). `INBOX.Entw&APw-rfe` with `.` is `Entwürfe`.
pub fn display_name(path: &str, delimiter: Option<char>) -> String {
    let last = match delimiter {
        Some(delimiter) => path.rsplit(delimiter).next().unwrap_or(path),
        None => path,
    };
    decode_utf7(last)
}

/// Modified UTF-7 (RFC 3501 5.1.3) decoded to text, for showing only: `&-` is `&`,
/// `&APw-` is `ü`. A part that does not decode is shown as it came, since a strange name is
/// better than a missing one.
pub fn decode_utf7(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find('&') {
        out.push_str(&rest[..start]);
        let shifted = &rest[start + 1..];
        let Some(end) = shifted.find('-') else {
            out.push_str(&rest[start..]);
            return out;
        };
        let encoded = &shifted[..end];
        match encoded {
            "" => out.push('&'),
            _ => match decode_shifted(encoded) {
                Some(decoded) => out.push_str(&decoded),
                None => out.push_str(&rest[start..start + 1 + end + 1]),
            },
        }
        rest = &shifted[end + 1..];
    }
    out.push_str(rest);
    out
}

/// Modified base64 (`,` for `/`, no padding) over UTF-16 big endian.
fn decode_shifted(encoded: &str) -> Option<String> {
    let mut bits: u32 = 0;
    let mut count = 0;
    let mut bytes = Vec::new();
    for byte in encoded.bytes() {
        let value = match byte {
            b'A'..=b'Z' => byte - b'A',
            b'a'..=b'z' => byte - b'a' + 26,
            b'0'..=b'9' => byte - b'0' + 52,
            b'+' => 62,
            b',' => 63,
            _ => return None,
        };
        bits = (bits << 6) | u32::from(value);
        count += 6;
        if count >= 8 {
            count -= 8;
            bytes.push(((bits >> count) & 0xff) as u8);
            bits &= (1 << count) - 1;
        }
    }
    if bytes.is_empty() || bytes.len() % 2 != 0 {
        return None;
    }
    let units: Vec<u16> = bytes.chunks_exact(2).map(|pair| u16::from_be_bytes([pair[0], pair[1]])).collect();
    String::from_utf16(&units).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn modified_utf7_decodes_for_display_and_keeps_what_does_not_decode() {
        // The cases of TorroMail's contracts/provider-discovery.json (`mailbox_names`).
        let cases = [
            ("INBOX", "INBOX"),
            ("Entw&APw-rfe", "Entwürfe"),
            ("Gel&APY-schte Elemente", "Gelöschte Elemente"),
            ("Tom &- Jerry", "Tom & Jerry"),
            ("&U,BTFw-", "台北"),
            ("&ZeVnLIqe-/&U,BTFw-", "日本語/台北"),
            ("broken &AP", "broken &AP"),
            ("&!!!-", "&!!!-"),
            ("Sent", "Sent"),
        ];
        for (input, expected) in cases {
            assert_eq!(decode_utf7(input), expected, "{input}");
        }
    }

    #[test]
    fn display_names_are_the_decoded_last_part_of_the_path() {
        assert_eq!(display_name("INBOX.Entw&APw-rfe", Some('.')), "Entwürfe");
        assert_eq!(display_name("[Gmail]/Sent Mail", Some('/')), "Sent Mail");
        assert_eq!(display_name("A.B", None), "A.B");
    }

    #[test]
    fn known_names_match_the_last_part_in_any_case_and_language() {
        assert_eq!(known_name("INBOX.Entw&APw-rfe", Some('.')), Some((Role::Drafts, 2)));
        assert_eq!(known_name("[Gmail]/Sent Mail", Some('/')), Some((Role::Sent, 1)));
        assert_eq!(known_name("GESENDETE OBJEKTE", None), Some((Role::Sent, 6)));
        assert_eq!(known_name("Gel&APY-schte Elemente", Some('/')), Some((Role::Trash, 5)));
        assert_eq!(known_name("spam", Some('/')), Some((Role::Junk, 1)));
        assert_eq!(known_name("Projects/Sent ideas", Some('/')), None);
    }

    #[test]
    fn attributes_name_roles_including_the_xlist_ones() {
        let attributes = |list: &[&str]| list.iter().map(|a| (*a).to_owned()).collect::<Vec<_>>();
        assert_eq!(role_of_attributes(&attributes(&["\\HasNoChildren", "\\Sent"])), Some(Role::Sent));
        assert_eq!(role_of_attributes(&attributes(&["\\Spam"])), Some(Role::Junk));
        assert_eq!(role_of_attributes(&attributes(&["\\inbox"])), Some(Role::Inbox));
        assert_eq!(role_of_attributes(&attributes(&["\\All", "\\HasChildren"])), None);
    }

    #[test]
    fn gmail_and_outlook_keep_their_own_sent_copies() {
        assert!(keeps_sent_copies("imap.gmail.com") && keeps_sent_copies("IMAP.GoogleMail.com."));
        assert!(keeps_sent_copies("outlook.office365.com") && keeps_sent_copies("imap-mail.outlook.com"));
        assert!(!keeps_sent_copies("imap.mailbox.org") && !keeps_sent_copies("notgmail.com.example"));
    }
}
