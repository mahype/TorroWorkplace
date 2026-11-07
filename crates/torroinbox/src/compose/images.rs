//! Local image input only (issue #461): dialogs return paths; screenshot pixels are encoded on
//! a worker and uploaded to the service. Clipboard formats are read serially, once per paste.

use super::ComposeError;
use std::path::PathBuf;

/// Debug reveals neither pixels nor clipboard text.
#[derive(Clone)]
pub(crate) enum Clipboard {
    Files(Vec<PathBuf>),
    Html { html: String, text: Option<String> },
    Image(Vec<u8>),
    Text(String),
    Empty,
    Unavailable,
}

impl std::fmt::Debug for Clipboard {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Clipboard(…)")
    }
}

pub(super) async fn choose() -> Option<PathBuf> {
    rfd::AsyncFileDialog::new()
        .set_title(super::tr("Insert image"))
        .add_filter(super::tr("Images"), &["png", "jpg", "jpeg", "gif", "webp"])
        .pick_file()
        .await
        .map(|f| f.path().to_path_buf())
}

trait Reader {
    fn files(&mut self) -> Result<Vec<PathBuf>, arboard::Error>;
    fn html(&mut self) -> Result<String, arboard::Error>;
    fn text(&mut self) -> Result<String, arboard::Error>;
    fn image(&mut self) -> Result<arboard::ImageData<'static>, arboard::Error>;
}

impl Reader for arboard::Clipboard {
    fn files(&mut self) -> Result<Vec<PathBuf>, arboard::Error> {
        self.get().file_list()
    }
    fn html(&mut self) -> Result<String, arboard::Error> {
        self.get().html()
    }
    fn text(&mut self) -> Result<String, arboard::Error> {
        self.get_text()
    }
    fn image(&mut self) -> Result<arboard::ImageData<'static>, arboard::Error> {
        self.get_image()
    }
}

fn optional<T>(result: Result<T, arboard::Error>) -> Result<Option<T>, ComposeError> {
    match result {
        Ok(value) => Ok(Some(value)),
        Err(arboard::Error::ContentNotAvailable) => Ok(None),
        Err(_) => Err(failed()),
    }
}

fn read(reader: &mut impl Reader, plain: bool) -> Result<Clipboard, ComposeError> {
    if plain {
        if let Some(text) = optional(reader.text())? {
            return Ok(Clipboard::Text(text));
        }
        if let Some(html) = optional(reader.html())? {
            let converted = torro_mail::html::prepare_paste(Some(&html), None).map_err(|_| failed())?;
            return Ok(Clipboard::Text(converted.plain));
        }
        return Ok(Clipboard::Empty);
    }
    if let Some(files) = optional(reader.files())?.filter(|files| !files.is_empty()) {
        if !files.iter().all(|path| local_file(path)) {
            return Err(failed());
        }
        return Ok(Clipboard::Files(files));
    }
    if let Some(html) = optional(reader.html())?.filter(|html| !html.trim().is_empty()) {
        let text = optional(reader.text())?;
        return Ok(Clipboard::Html { html, text });
    }
    if let Some(pixels) = optional(reader.image())? {
        if pixels.width == 0
            || pixels.height == 0
            || pixels.width > 32768
            || pixels.height > 32768
            || pixels.width.saturating_mul(pixels.height) > 32_000_000
        {
            return Err(failed());
        }
        let mut png = std::io::Cursor::new(Vec::new());
        let image = image::RgbaImage::from_raw(pixels.width as u32, pixels.height as u32, pixels.bytes.into_owned())
            .ok_or_else(failed)?;
        image::DynamicImage::ImageRgba8(image).write_to(&mut png, image::ImageFormat::Png).map_err(|_| failed())?;
        if png.get_ref().len() as u64 > torro_ipc::mail::MAX_FILE_BYTES {
            return Err(failed());
        }
        return Ok(Clipboard::Image(png.into_inner()));
    }
    Ok(optional(reader.text())?.map_or(Clipboard::Empty, Clipboard::Text))
}

fn local_file(path: &std::path::Path) -> bool {
    #[cfg(windows)]
    if !matches!(path.components().next(), Some(std::path::Component::Prefix(prefix))
        if matches!(prefix.kind(), std::path::Prefix::Disk(_) | std::path::Prefix::VerbatimDisk(_)))
    {
        // A native file list is not permission to visit UNC servers or device namespaces.
        return false;
    }
    path.is_absolute()
        && path.ancestors().all(|part| std::fs::symlink_metadata(part).is_ok_and(|m| !m.file_type().is_symlink()))
        && std::fs::symlink_metadata(path).is_ok_and(|m| m.is_file())
}

pub(super) async fn clipboard(plain: bool) -> Result<Clipboard, ComposeError> {
    tokio::task::spawn_blocking(move || {
        let Ok(mut clipboard) = arboard::Clipboard::new() else { return Ok(Clipboard::Unavailable) };
        read(&mut clipboard, plain)
    })
    .await
    .map_err(|_| failed())?
}

fn failed() -> ComposeError {
    ComposeError::Other(super::tr("Couldn't read the clipboard.").into())
}

pub(super) fn dropped(event: iced::Event, _: iced::event::Status, _: iced::window::Id) -> Option<super::Message> {
    match event {
        iced::Event::Window(iced::window::Event::FileDropped(path)) => Some(super::Message::ImagePath(Some(path))),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[derive(Default)]
    struct Fake {
        files: Option<Vec<PathBuf>>,
        html: Option<String>,
        text: Option<String>,
        calls: Vec<&'static str>,
        pixels: bool,
    }
    impl Reader for Fake {
        fn files(&mut self) -> Result<Vec<PathBuf>, arboard::Error> {
            self.calls.push("files");
            self.files.clone().ok_or(arboard::Error::ContentNotAvailable)
        }
        fn html(&mut self) -> Result<String, arboard::Error> {
            self.calls.push("html");
            self.html.clone().ok_or(arboard::Error::ContentNotAvailable)
        }
        fn text(&mut self) -> Result<String, arboard::Error> {
            self.calls.push("text");
            self.text.clone().ok_or(arboard::Error::ContentNotAvailable)
        }
        fn image(&mut self) -> Result<arboard::ImageData<'static>, arboard::Error> {
            self.calls.push("image");
            if self.pixels {
                Ok(arboard::ImageData { width: 2, height: 2, bytes: std::borrow::Cow::Owned(vec![255; 16]) })
            } else {
                Err(arboard::Error::ContentNotAvailable)
            }
        }
    }
    #[test]
    fn native_files_then_html_then_image_then_text_and_plain_never_reads_files_or_images() {
        let dir = tempfile::tempdir().expect("temp");
        let file = dir.path().join("image.png");
        std::fs::write(&file, b"file").expect("file");
        let file = std::fs::canonicalize(file).expect("canonical");
        let mut fake = Fake {
            files: Some(vec![file.clone()]),
            html: Some(" <b>Rich</b> ".into()),
            text: Some("Text".into()),
            ..Fake::default()
        };
        assert!(matches!(read(&mut fake, false), Ok(Clipboard::Files(paths)) if paths == [file]));
        assert_eq!(fake.calls, ["files"]);
        fake.calls.clear();
        assert!(matches!(read(&mut fake, true), Ok(Clipboard::Text(s)) if s == "Text"));
        assert_eq!(fake.calls, ["text"]);
        fake.files = None;
        fake.calls.clear();
        assert!(matches!(read(&mut fake, false), Ok(Clipboard::Html { .. })));
        assert_eq!(fake.calls, ["files", "html", "text"]);
        fake.text = None;
        fake.calls.clear();
        assert!(matches!(read(&mut fake, true), Ok(Clipboard::Text(s)) if s == " Rich "));
        assert_eq!(fake.calls, ["text", "html"]);
        fake.html = None;
        fake.calls.clear();
        assert!(matches!(read(&mut fake, false), Ok(Clipboard::Empty)));
        assert_eq!(fake.calls, ["files", "html", "image", "text"]);
        fake.files = Some(vec![PathBuf::from("relative.png")]);
        assert!(read(&mut fake, false).is_err());
    }

    #[test]
    fn screenshot_is_encoded_for_the_existing_chunk_upload_and_never_double_pastes_text() {
        let mut fake = Fake { pixels: true, text: Some("Alternative".into()), ..Fake::default() };
        let Clipboard::Image(bytes) = read(&mut fake, false).expect("read") else { panic!("PNG") };
        assert!(bytes.starts_with(b"\x89PNG\r\n\x1a\n"));
        assert_eq!(fake.calls, ["files", "html", "image"]);
        fake.calls.clear();
        assert!(matches!(read(&mut fake, true), Ok(Clipboard::Text(s)) if s == "Alternative"));
        assert_eq!(fake.calls, ["text"]);
    }

    #[cfg(unix)]
    #[test]
    fn native_file_lists_reject_symlinks_including_parent_components() {
        let dir = tempfile::tempdir().expect("temp");
        let root = std::fs::canonicalize(dir.path()).expect("canonical");
        let file = root.join("file");
        std::fs::write(&file, b"file").expect("write");
        let link = root.join("link");
        std::os::unix::fs::symlink(&file, &link).expect("symlink");
        assert!(!local_file(&link));
        let folder = root.join("folder-link");
        std::os::unix::fs::symlink(&root, &folder).expect("symlink");
        assert!(!local_file(&folder.join("file")));
        assert!(local_file(&file));
    }
}
