//! What the windows ask of the computer itself (stage 4 of `docs/chat.md`): open a file or a link
//! with the system's program for it, the system's dialogs to choose files and a place to save,
//! the clipboard, and the address of a file on the account's Nextcloud.
//!
//! Nothing here talks to the core. A file the core fetched (`chat.get_attachment`) is opened here,
//! as `torroinbox` opens mail attachments (decision 22: videos play in the system's player).

use std::path::{Path, PathBuf};
use std::process::Command;

use torro_core::i18n::tr;

/// What a paste brought ([`clipboard`]). `Debug` shows neither text nor pixels.
#[derive(Clone)]
pub enum Clip {
    /// Files copied in a file manager.
    Files(Vec<PathBuf>),
    /// A picture (a screenshot), encoded as PNG.
    Picture(Vec<u8>),
    /// Text: it goes into the field as usual.
    Text(String),
    /// Nothing to paste.
    Empty,
}

impl std::fmt::Debug for Clip {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Clip(…)")
    }
}

/// The command that opens `target` (a path or an `https` link) with the system's program for it:
/// `open` on macOS, the Explorer on Windows, `xdg-open` elsewhere.
fn opener(target: &str) -> Command {
    #[cfg(target_os = "macos")]
    let program = "open";
    #[cfg(target_os = "windows")]
    let program = "explorer";
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    let program = "xdg-open";
    let mut command = Command::new(program);
    command.arg(target);
    command
}

/// A file whose name says it could run a program when opened: its folder opens instead (as Mail
/// does with such attachments). Without an extension nobody can tell, so it counts as one.
pub fn runs_programs(name: &str) -> bool {
    const RISKY: [&str; 30] = [
        "app", "appimage", "apk", "bat", "bin", "cmd", "com", "command", "cpl", "deb", "desktop", "dmg", "exe", "hta",
        "jar", "js", "jse", "lnk", "msi", "pkg", "ps1", "py", "rpm", "run", "scr", "sh", "url", "vbs", "workflow",
        "wsf",
    ];
    let extension = Path::new(name).extension().and_then(|ext| ext.to_str()).map(str::to_ascii_lowercase);
    extension.is_none_or(|ext| RISKY.contains(&ext.as_str()))
}

/// Opens the file at `path` with the system's program for it, or its folder for a file that could
/// run a program ([`runs_programs`]). An error is a sentence for people.
pub fn open_file(path: &str) -> Result<(), String> {
    let path = Path::new(path);
    let name = path.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default();
    let target = match path.parent() {
        Some(folder) if runs_programs(&name) => folder.to_path_buf(),
        _ => path.to_path_buf(),
    };
    opener(&target.to_string_lossy()).spawn().map(|_| ()).map_err(|_| tr("The file could not be opened.").to_owned())
}

/// Opens `link` in the browser – only `https` and `http`, so a text cannot start a program.
pub fn open_link(link: &str) -> Result<(), String> {
    if !(link.starts_with("https://") || link.starts_with("http://")) {
        return Err(tr("This link cannot be opened.").to_owned());
    }
    opener(link).spawn().map(|_| ()).map_err(|_| tr("The link could not be opened.").to_owned())
}

/// The host of an account (`nextcloud:<login>@<host>[/path]`, the login may hold an `@`), with
/// the path of a server in a subdirectory: `cloud.example.org` or `example.org/cloud`.
pub fn account_server(account: &str) -> Option<&str> {
    let rest = account.strip_prefix("nextcloud:")?;
    let (_, server) = rest.rsplit_once('@')?;
    (!server.is_empty()).then_some(server)
}

/// The address of the file `file` (its id on the server) on the account's Nextcloud, for "Open in
/// Nextcloud": `https://<server>/f/<id>`.
pub fn nextcloud_file(account: &str, file: &str) -> Option<String> {
    let server = account_server(account)?;
    (!file.is_empty() && file.bytes().all(|byte| byte.is_ascii_alphanumeric()))
        .then(|| format!("https://{}/f/{file}", server.trim_end_matches('/')))
}

/// The host of `link` (`https://example.com/a` → `example.com`), lower case.
pub fn link_host(link: &str) -> Option<String> {
    let rest = link.strip_prefix("https://").or_else(|| link.strip_prefix("http://"))?;
    let host = rest.split(['/', '?', '#']).next()?.rsplit('@').next()?.split(':').next()?;
    (!host.is_empty()).then(|| host.to_ascii_lowercase())
}

/// Whether `link` goes to the account's own server: those links get their card from the core by
/// themselves, every other one as Settings › Privacy says (decision 23, issue #672).
pub fn is_own_link(account: &str, link: &str) -> bool {
    let own = account_server(account).and_then(|server| server.split('/').next()).map(str::to_ascii_lowercase);
    own.is_some_and(|own| link_host(link).as_deref() == Some(own.as_str()))
}

/// The files a person picks in the system's dialog ("Attach file"); empty when cancelled.
pub async fn choose_files() -> Vec<PathBuf> {
    let chosen = rfd::AsyncFileDialog::new().set_title(tr("Attach files")).pick_files().await;
    chosen.unwrap_or_default().into_iter().map(|file| file.path().to_path_buf()).collect()
}

/// Where a person saves the file `name` ("Save as…"); `None` when cancelled.
pub async fn choose_save_file(name: String) -> Option<PathBuf> {
    let dialog = rfd::AsyncFileDialog::new().set_title(tr("Save file")).set_file_name(name);
    dialog.save_file().await.map(|file| file.path().to_path_buf())
}

/// What the clipboard holds for a paste: files first, then a picture, then text. Read on a worker
/// thread; an error is a sentence for people.
pub async fn clipboard() -> Result<Clip, String> {
    let failed = || tr("The clipboard could not be read.").to_owned();
    tokio::task::spawn_blocking(move || {
        let Ok(mut clipboard) = arboard::Clipboard::new() else { return Err(failed()) };
        if let Ok(files) = clipboard.get().file_list()
            && !files.is_empty()
        {
            return Ok(Clip::Files(files.into_iter().filter(|path| path.is_absolute()).collect()));
        }
        if let Ok(pixels) = clipboard.get_image() {
            return png_of(pixels.width, pixels.height, pixels.bytes.into_owned())
                .map(Clip::Picture)
                .ok_or_else(failed);
        }
        Ok(clipboard.get_text().map_or(Clip::Empty, Clip::Text))
    })
    .await
    .map_err(|_| failed())?
}

/// The picture of `bytes` (PNG, JPEG, GIF, WebP) ready to draw, decoded here: a handle of pixels shows
/// in the very next frame, where one of encoded bytes waits for the renderer to decode it (and the
/// pictures without a window, `--shots`, would stay empty). `None` for bytes that are no picture.
pub fn picture_handle(bytes: &[u8]) -> Option<iced::widget::image::Handle> {
    let pixels = image::load_from_memory(bytes).ok()?.into_rgba8();
    let (width, height) = pixels.dimensions();
    Some(iced::widget::image::Handle::from_rgba(width, height, pixels.into_raw()))
}

/// RGBA pixels as PNG; `None` for a picture of no or absurd size, or one over the 25 MiB the core
/// takes in pieces.
fn png_of(width: usize, height: usize, rgba: Vec<u8>) -> Option<Vec<u8>> {
    if width == 0 || height == 0 || width > 32768 || height > 32768 {
        return None;
    }
    let pixels = image::RgbaImage::from_raw(u32::try_from(width).ok()?, u32::try_from(height).ok()?, rgba)?;
    let mut png = std::io::Cursor::new(Vec::new());
    image::DynamicImage::ImageRgba8(pixels).write_to(&mut png, image::ImageFormat::Png).ok()?;
    let png = png.into_inner();
    (png.len() as u64 <= torro_ipc::chat::MAX_UPLOAD_BYTES).then_some(png)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_server_of_an_account_names_its_files() {
        let account = "nextcloud:anna@cloud.example.org";
        assert_eq!(nextcloud_file(account, "5103").as_deref(), Some("https://cloud.example.org/f/5103"));
        let sub = "nextcloud:anna@firm.org@example.org/cloud";
        assert_eq!(nextcloud_file(sub, "7").as_deref(), Some("https://example.org/cloud/f/7"));
        assert_eq!(nextcloud_file(account, "../x"), None, "an id is no path");
        assert_eq!(nextcloud_file("mail:anna@x", "7"), None);
    }

    #[test]
    fn own_links_are_told_from_foreign_ones() {
        let account = "nextcloud:anna@cloud.example.org";
        assert!(is_own_link(account, "https://cloud.example.org/s/abc"));
        assert!(is_own_link(account, "https://CLOUD.example.org:443/x"));
        assert!(!is_own_link(account, "https://example.com/blog"));
        assert!(!is_own_link(account, "https://cloud.example.org.evil.net/x"));
        assert_eq!(link_host("https://user@host.org/a?b").as_deref(), Some("host.org"));
    }

    #[test]
    fn only_web_links_open_and_programs_open_their_folder() {
        assert!(open_link("file:///etc/passwd").is_err());
        assert!(open_link("javascript:alert(1)").is_err());
        assert!(runs_programs("setup.exe") && runs_programs("README"));
        assert!(!runs_programs("Offer.pdf"));
    }

    #[test]
    fn pixels_become_a_png() {
        let png = png_of(2, 2, vec![255; 16]).expect("a picture");
        assert!(png.starts_with(&[0x89, b'P', b'N', b'G']));
        assert_eq!(png_of(0, 2, Vec::new()), None);
    }
}
