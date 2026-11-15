//! Files of the own Nextcloud server (issue #790, `docs/inhaltskarten.md` "Nextcloud-Datei"): their links, and – with
//! strand 2c of issue #784 – their cards (name, size, folder, the server's preview), without Settings › Privacy,
//! since no foreign provider is asked; only with read access to `files` for the reader.

use super::LinkKind;

/// What `url` on the own server `own` names as a file: `/f/<id>`, `/s/<token>`, `/index.php/apps/files?dir=…`
/// ([`super::FileLink`]); `None` for any other address. `url` is on `own` already ([`super::classify`] checked).
///
/// Strand 2c (issue #790) fills it in; until then no address is a file.
pub fn parse(url: &url::Url, own: &str) -> Option<LinkKind> {
    let _ = (url, own);
    None
}
